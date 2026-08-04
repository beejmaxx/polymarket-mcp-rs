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

    let tools = client.peer().list_tools(None).await.unwrap();
    let mut names = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["get_market", "search_markets", "server_status"]);

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
