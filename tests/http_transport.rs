use std::time::Duration;

use polymarket_mcp_rs::{
    App, ToolProfile,
    http::{HttpConfig, serve_on_listener},
};
use rmcp::{
    ServiceExt, model::ReadResourceRequestParams, transport::StreamableHttpClientTransport,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn streamable_http_serves_compact_credential_free_profile() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = HttpConfig::from_environment(&address.to_string()).unwrap();
    config.allowed_hosts = vec!["127.0.0.1".to_owned()];
    config.requests_per_minute = 100;
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let server = tokio::spawn(serve_on_listener(
        listener,
        App::new_public().unwrap(),
        ToolProfile::Chatgpt,
        config,
        cancellation.clone(),
        async move { server_cancellation.cancelled_owned().await },
    ));

    let health: serde_json::Value = reqwest::get(format!("http://{address}/healthz"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["profile"], "chatgpt");

    let transport = StreamableHttpClientTransport::from_uri(format!("http://{address}/mcp"));
    let client = ().serve(transport).await.unwrap();
    let tools = client.peer().list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 11);
    assert!(
        tools
            .tools
            .iter()
            .any(|tool| tool.name == "get_market_brief")
    );
    assert!(!tools.tools.iter().any(|tool| tool.name == "watch_markets"));
    assert!(!tools.tools.iter().any(|tool| tool.name == "place_order"));
    let brief_tool = tools
        .tools
        .iter()
        .find(|tool| tool.name == "get_market_brief")
        .unwrap();
    assert_eq!(
        brief_tool.meta.as_ref().unwrap()["ui"]["resourceUri"],
        "ui://polymarket/market-brief-v1.html"
    );
    let resources = client.peer().list_resources(None).await.unwrap();
    assert_eq!(resources.resources.len(), 1);
    let resource = client
        .peer()
        .read_resource(ReadResourceRequestParams::new(
            "ui://polymarket/market-brief-v1.html",
        ))
        .await
        .unwrap();
    assert_eq!(resource.contents.len(), 1);
    assert!(format!("{:?}", resource.contents[0]).contains("ui/notifications/tool-result"));
    client.cancel().await.unwrap();

    let metrics = reqwest::get(format!("http://{address}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics.contains("polymarket_mcp_http_requests_total"));

    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn public_http_rejects_stateful_or_credentialed_profiles() {
    for profile in [
        ToolProfile::Research,
        ToolProfile::Trading,
        ToolProfile::All,
    ] {
        let error = polymarket_mcp_rs::http::serve_http(
            profile,
            HttpConfig::from_environment("127.0.0.1:0").unwrap(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code(), "invalid_input");
    }
}
