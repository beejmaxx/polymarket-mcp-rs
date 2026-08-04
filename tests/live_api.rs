use polymarket_mcp_rs::{App, types::ListMarketsInput};
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
            sort_by: Some("volume_24h".to_owned()),
            ascending: Some(false),
        })
        .await
        .unwrap();
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

    let history = app
        .get_price_history(token_id.clone(), Some("1d".to_owned()), None, None, Some(5))
        .await
        .unwrap();
    assert!(!history.points.is_empty());

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
        }
    }
}
