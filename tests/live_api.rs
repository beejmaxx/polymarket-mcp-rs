use polymarket_mcp_rs::App;

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
