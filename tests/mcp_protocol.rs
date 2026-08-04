use polymarket_mcp_rs::{App, PolymarketServer};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::json;

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
            "analyze_order_book",
            "analyze_wallet_risk",
            "compare_markets",
            "get_event",
            "get_live_snapshot",
            "get_market",
            "get_market_holders",
            "get_order_book",
            "get_price_history",
            "get_realtime_events",
            "get_realtime_status",
            "get_wallet_activity",
            "get_wallet_positions",
            "get_wallet_trades",
            "get_wallet_value",
            "list_markets",
            "list_recordings",
            "replay_events",
            "replay_market",
            "scan_market_microstructure",
            "search_markets",
            "server_status",
            "simulate_order",
            "start_recording",
            "stop_recording",
            "stop_watching",
            "watch_markets"
        ]
    );
    assert!(!names.contains(&"place_order".to_owned()));

    let status = client
        .peer()
        .call_tool(CallToolRequestParams::new("server_status"))
        .await
        .unwrap();
    assert_eq!(status.is_error, Some(false));
    assert!(status.structured_content.is_some());

    let invalid =
        client
            .peer()
            .call_tool(CallToolRequestParams::new("search_markets").with_arguments(
                serde_json::from_value(json!({"query": "   ", "limit": 3})).unwrap(),
            ))
            .await
            .unwrap();
    assert_eq!(invalid.is_error, Some(true));
    let error = invalid.structured_content.expect("typed tool error");
    assert_eq!(error["code"], "invalid_input");
    assert_eq!(error["retryable"], false);

    client.cancel().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn compiled_stdio_binary_initializes_and_serves_tools() {
    let directory = tempfile::tempdir().unwrap();
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_polymarket-mcp-rs")).configure(
            |command| {
                command
                    .current_dir(directory.path())
                    .env("POLYMARKET_MCP_DB", directory.path().join("stdio.sqlite3"));
            },
        ),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let peer_info = client.peer().peer_info().expect("stdio server info");
    assert_eq!(
        peer_info
            .server_info
            .as_ref()
            .map(|info| info.name.as_str()),
        Some("polymarket-mcp-rs")
    );
    let tools = client.peer().list_tools(None).await.unwrap();
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "scan_market_microstructure")
    );
    assert!(!tools.tools.iter().any(|tool| tool.name == "place_order"));
    let status = client
        .peer()
        .call_tool(CallToolRequestParams::new("server_status"))
        .await
        .unwrap();
    assert_eq!(status.is_error, Some(false));

    client.cancel().await.unwrap();
}
