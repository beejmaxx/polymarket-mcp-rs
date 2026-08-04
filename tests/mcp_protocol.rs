use polymarket_mcp_rs::{App, PolymarketServer};
use rmcp::{ServiceExt, model::CallToolRequestParams};

#[tokio::test]
async fn client_can_initialize_list_tools_and_call_offline_status() {
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = PolymarketServer::new(App::new().unwrap());

    let server_task = tokio::spawn(async move {
        let service = server.serve(server_transport).await.unwrap();
        service.waiting().await.unwrap();
    });
    let client = ().serve(client_transport).await.unwrap();
    let peer_info = client
        .peer()
        .peer_info()
        .expect("server initialization info");
    assert_eq!(
        peer_info
            .server_info
            .as_ref()
            .map(|info| info.name.as_str()),
        Some("polymarket-mcp-rs")
    );

    let tools = client.peer().list_tools(None).await.unwrap();
    let mut names = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            "analyze_wallet_risk",
            "cancel_all_orders",
            "cancel_market_orders",
            "cancel_order",
            "compare_markets",
            "get_balance_allowance",
            "get_event",
            "get_live_snapshot",
            "get_market",
            "get_market_holders",
            "get_order",
            "get_order_approval",
            "get_order_book",
            "get_price_history",
            "get_realtime_events",
            "get_realtime_status",
            "get_wallet_activity",
            "get_wallet_positions",
            "get_wallet_trades",
            "get_wallet_value",
            "list_account_trades",
            "list_markets",
            "list_open_orders",
            "list_recordings",
            "place_batch_orders",
            "place_order",
            "preview_order",
            "replay_market",
            "search_markets",
            "server_status",
            "simulate_order",
            "start_recording",
            "stop_recording",
            "stop_watching",
            "trading_status",
            "watch_markets"
        ]
    );

    let status = client
        .peer()
        .call_tool(CallToolRequestParams::new("server_status"))
        .await
        .unwrap();
    assert_eq!(status.is_error, Some(false));
    assert!(status.structured_content.is_some());

    client.cancel().await.unwrap();
    server_task.await.unwrap();
}
