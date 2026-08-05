use polymarket_mcp_rs::{App, PolymarketServer, ToolProfile};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::json;

#[test]
fn tools_command_does_not_create_a_database() {
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_polymarket-mcp-rs"))
        .arg("tools")
        .current_dir(directory.path())
        .env_remove("POLYMARKET_MCP_DB")
        .env("HOME", directory.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!directory.path().join("polymarket-mcp.sqlite3").exists());
    assert!(!directory.path().join("Library").exists());
}

#[tokio::test]
async fn client_can_initialize_list_tools_and_call_offline_status() {
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = PolymarketServer::new(App::new_ephemeral().unwrap());

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
            "analyze_event_consistency",
            "analyze_order_book",
            "analyze_wallet_risk",
            "compare_markets",
            "get_event",
            "get_live_snapshot",
            "get_market",
            "get_market_brief",
            "get_market_holders",
            "get_order_book",
            "get_price_history",
            "get_realtime_events",
            "get_realtime_status",
            "get_wallet_activity",
            "get_wallet_positions",
            "get_wallet_summary",
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

    let unknown_field = client
        .peer()
        .call_tool(CallToolRequestParams::new("search_markets").with_arguments(
            serde_json::from_value(json!({"query": "bitcoin", "bogus": true})).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(unknown_field.is_error, Some(true));

    client.cancel().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn authenticated_user_tools_dispatch_safely_without_credentials() {
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = PolymarketServer::with_profile(App::new_ephemeral().unwrap(), ToolProfile::All);
    let server_task = tokio::spawn(async move {
        let service = server.serve(server_transport).await.unwrap();
        service.waiting().await.unwrap();
    });
    let client = ().serve(client_transport).await.unwrap();

    let status = client
        .peer()
        .call_tool(CallToolRequestParams::new("get_user_realtime_status"))
        .await
        .unwrap();
    assert_eq!(status.is_error, Some(false));
    assert_eq!(status.structured_content.unwrap()["active_watch_count"], 0);

    let watch = client
        .peer()
        .call_tool(
            CallToolRequestParams::new("watch_user_events").with_arguments(
                serde_json::from_value(json!({
                    "condition_ids": [
                        "0x0000000000000000000000000000000000000000000000000000000000000001"
                    ]
                }))
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(watch.is_error, Some(true));
    assert_eq!(watch.structured_content.unwrap()["code"], "invalid_input");

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
