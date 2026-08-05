use polymarket_mcp_rs::{
    App,
    types::{AnalyzeOrderBookInput, ListMarketsInput, ScanMarketMicrostructureInput},
};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires live Polymarket APIs"]
async fn search_then_fetch_market_from_production() {
    let app = App::new().unwrap();
    let search = app
        .search_markets("bitcoin".to_owned(), Some(5))
        .await
        .unwrap();
    let first = search
        .markets
        .first()
        .expect("production search should return at least one Bitcoin market");

    let market = app.get_market(first.market_id.clone()).await.unwrap();
    assert_eq!(market.market_id, first.market_id);
    assert!(!market.outcomes.is_empty());
}

#[tokio::test]
#[ignore = "requires live Polymarket APIs"]
async fn compiled_stdio_binary_serves_live_structured_tools() {
    let directory = tempfile::tempdir().unwrap();
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_polymarket-mcp-rs")).configure(
            |command| {
                command.current_dir(directory.path()).env(
                    "POLYMARKET_MCP_DB",
                    directory.path().join("stdio-live.sqlite3"),
                );
            },
        ),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let search = client
        .peer()
        .call_tool(CallToolRequestParams::new("search_markets").with_arguments(
            serde_json::from_value(json!({"query": "bitcoin", "limit": 3})).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(search.is_error, Some(false));
    let search = search.structured_content.expect("typed search response");
    assert!(search["count"].as_u64().unwrap_or_default() > 0);
    let market_id = search["markets"][0]["market_id"]
        .as_str()
        .expect("search market ID");
    let brief = client
        .peer()
        .call_tool(
            CallToolRequestParams::new("get_market_brief").with_arguments(
                serde_json::from_value(json!({
                    "market_id": market_id,
                    "sample_shares": "10",
                    "history_limit": 10
                }))
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(brief.is_error, Some(false));
    let brief = brief.structured_content.expect("typed market brief");
    assert_eq!(brief["market"]["market_id"], market_id);
    assert!(
        brief["sources"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let scan = client
        .peer()
        .call_tool(
            CallToolRequestParams::new("scan_market_microstructure")
                .with_arguments(serde_json::from_value(json!({"limit": 2, "depth": 5})).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(scan.is_error, Some(false));
    let scan = scan.structured_content.expect("typed scanner response");
    assert!(scan["market_count"].as_u64().unwrap_or_default() > 0);
    assert!(scan["outcome_book_count"].as_u64().unwrap_or_default() > 0);

    client.cancel().await.unwrap();
}

#[tokio::test]
#[ignore = "requires live Polymarket APIs"]
async fn discovery_books_history_and_holders_work_in_production() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new_with_database(directory.path().join("live.sqlite3")).unwrap();
    let listed = app
        .list_markets(ListMarketsInput {
            limit: Some(10),
            offset: None,
            tag_slug: None,
            featured: None,
            min_liquidity: Some("1000".to_owned()),
            min_volume: None,
            end_after: None,
            end_before: None,
            sort_by: Some("volume_24h".to_owned()),
            ascending: Some(false),
        })
        .await
        .unwrap();
    assert_eq!(listed.count, listed.markets.len());
    assert!(listed.markets.windows(2).all(|markets| {
        let left = markets[0]
            .volume_24h
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok());
        let right = markets[1]
            .volume_24h
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok());
        match (left, right) {
            (Some(left), Some(right)) => left >= right,
            (Some(_), None) | (None, None) => true,
            (None, Some(_)) => false,
        }
    }));
    let featured = app
        .list_markets(ListMarketsInput {
            limit: Some(5),
            offset: None,
            tag_slug: None,
            featured: Some(true),
            min_liquidity: None,
            min_volume: None,
            end_after: None,
            end_before: None,
            sort_by: Some("volume_24h".to_owned()),
            ascending: Some(false),
        })
        .await
        .unwrap();
    assert_eq!(featured.count, featured.markets.len());
    let second_page = app
        .list_markets(ListMarketsInput {
            limit: Some(10),
            offset: Some(10),
            tag_slug: None,
            featured: None,
            min_liquidity: Some("1000".to_owned()),
            min_volume: None,
            end_after: None,
            end_before: None,
            sort_by: Some("volume_24h".to_owned()),
            ascending: Some(false),
        })
        .await
        .unwrap();
    assert!(listed.markets.iter().all(|left| {
        second_page
            .markets
            .iter()
            .all(|right| left.market_id != right.market_id)
    }));
    let summary = listed
        .markets
        .iter()
        .find(|market| !market.outcomes.is_empty())
        .expect("active production markets should include outcomes");

    let event = app
        .get_event(Some(summary.event_id.clone()), None)
        .await
        .unwrap();
    assert_eq!(event.event_id, summary.event_id);
    let consistency = app
        .analyze_event_consistency(polymarket_mcp_rs::types::AnalyzeEventConsistencyInput {
            event_id: Some(summary.event_id.clone()),
            slug: None,
        })
        .await
        .unwrap();
    assert_eq!(consistency.event_id, summary.event_id);

    let mut market = None;
    for candidate in &listed.markets {
        let detail = app.get_market(candidate.market_id.clone()).await.unwrap();
        if detail
            .outcomes
            .iter()
            .any(|outcome| outcome.order_book.is_some())
        {
            market = Some(detail);
            break;
        }
    }
    let market = market.expect("top active markets should contain a current CLOB order book");
    let token_id = market
        .outcomes
        .iter()
        .find(|outcome| outcome.order_book.is_some())
        .and_then(|outcome| outcome.token_id.clone())
        .expect("an active order-book market should expose token IDs");
    let book = app.get_order_book(token_id.clone(), Some(5)).await.unwrap();
    assert_eq!(book.summary.token_id, token_id);
    assert!(book.bids.len() <= 5);
    assert!(book.asks.len() <= 5);
    let analysis = app
        .analyze_order_book(AnalyzeOrderBookInput {
            token_id: token_id.clone(),
            depth: Some(10),
            price_band: Some("0.02".to_owned()),
            sample_shares: Some("10".to_owned()),
        })
        .await
        .unwrap();
    assert_eq!(analysis.token_id, token_id);
    assert_eq!(analysis.sample_buy.requested_shares, "10");
    assert_eq!(analysis.sample_sell.requested_shares, "10");

    let scan = app
        .scan_market_microstructure(ScanMarketMicrostructureInput {
            limit: Some(3),
            tag_slug: None,
            min_liquidity: None,
            depth: Some(10),
            price_band: Some("0.02".to_owned()),
            sample_shares: Some("25".to_owned()),
        })
        .await
        .unwrap();
    assert!(scan.market_count <= 3);
    assert!(scan.outcome_book_count > 0);

    let history = app
        .get_price_history(
            token_id.clone(),
            Some("1d".to_owned()),
            None,
            None,
            Some(5),
            Some(100),
        )
        .await
        .unwrap();
    assert!(!history.points.is_empty());
    assert!(history.points.len() <= 100);

    let watch = app.watch_markets(vec![token_id]).await.unwrap();
    let mut snapshot = app
        .get_live_snapshot(watch.watch_id.clone(), Some(5))
        .await
        .unwrap();
    for _ in 0..40 {
        if snapshot.watch.websocket_update_count > 0
            && snapshot
                .books
                .iter()
                .any(|book| book.source.starts_with("websocket"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        snapshot = app
            .get_live_snapshot(watch.watch_id.clone(), Some(5))
            .await
            .unwrap();
    }
    let status = app.get_realtime_status().await;
    assert!(
        snapshot.watch.websocket_update_count > 0
            && snapshot
                .books
                .iter()
                .any(|book| book.source.starts_with("websocket")),
        "websocket did not deliver a genuine update; status: {status:?}"
    );
    assert_eq!(status.active_watch_count, 1);
    let recording = app
        .start_recording(watch.watch_id.clone(), Some("live smoke".to_owned()))
        .await
        .unwrap();
    let stopped_recording = app
        .stop_recording(recording.recording_id.clone())
        .await
        .unwrap();
    assert!(stopped_recording.snapshot_count >= 1);
    let replay = app
        .replay_market(recording.recording_id, None, None, None, Some(10))
        .await
        .unwrap();
    assert!(!replay.books.is_empty());
    let simulation = app
        .simulate_order(
            snapshot.books[0].token_id.clone(),
            "buy".to_owned(),
            "1".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(simulation.requested_shares, "1");
    assert!(app.stop_watching(watch.watch_id).await.unwrap().stopped);

    if let Some(condition_id) = market.condition_id {
        let holders = app
            .get_market_holders(condition_id.clone(), Some(3), Some(1))
            .await
            .unwrap();
        assert_eq!(holders.condition_id, condition_id);
        if let Some(wallet) = holders
            .outcomes
            .iter()
            .flat_map(|outcome| &outcome.holders)
            .next()
            .map(|holder| holder.wallet.clone())
        {
            let positions = app
                .get_wallet_positions(wallet.clone(), Some(5), None)
                .await
                .unwrap();
            assert_eq!(positions.wallet, wallet);
            let value = app.get_wallet_value(wallet.clone()).await.unwrap();
            assert_eq!(value.wallet, wallet);
            let trades = app
                .get_wallet_trades(wallet.clone(), Some(5), None)
                .await
                .unwrap();
            assert_eq!(trades.wallet, wallet);
            let activity = app
                .get_wallet_activity(wallet.clone(), Some(5), None, None, None)
                .await
                .unwrap();
            assert_eq!(activity.wallet, wallet);
            let risk = app.analyze_wallet_risk(wallet.clone()).await.unwrap();
            assert_eq!(risk.wallet, wallet);
            let summary = app
                .get_wallet_summary(polymarket_mcp_rs::types::WalletSummaryInput {
                    wallet: wallet.clone(),
                    recent_limit: Some(3),
                })
                .await
                .unwrap();
            assert_eq!(summary.wallet, wallet);
            assert!(!summary.sources.is_empty());
        }
    }
}
