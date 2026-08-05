use polymarket_mcp_rs::{App, PolymarketServer, ToolProfile};

fn expected(contents: &str) -> Vec<&str> {
    contents.lines().filter(|line| !line.is_empty()).collect()
}

fn names(profile: ToolProfile) -> Vec<String> {
    PolymarketServer::with_profile(App::new_ephemeral().unwrap(), profile)
        .tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect()
}

#[test]
fn chatgpt_tool_catalog_matches_golden_contract() {
    assert_eq!(
        names(ToolProfile::Chatgpt),
        expected(include_str!("contracts/chatgpt-tools.txt"))
    );
}

#[test]
fn research_tool_catalog_matches_golden_contract() {
    assert_eq!(
        names(ToolProfile::Research),
        expected(include_str!("contracts/research-tools.txt"))
    );
}

#[test]
fn all_tool_catalog_matches_golden_contract() {
    assert_eq!(
        names(ToolProfile::All),
        expected(include_str!("contracts/all-tools.txt"))
    );
}

#[test]
fn every_tool_has_typed_input_and_output_contracts() {
    for tool in
        PolymarketServer::with_profile(App::new_ephemeral().unwrap(), ToolProfile::All).tools()
    {
        assert_eq!(
            tool.input_schema
                .get("type")
                .and_then(|value| value.as_str()),
            Some("object"),
            "{} must accept an object schema",
            tool.name
        );
        if tool
            .input_schema
            .get("properties")
            .and_then(|value| value.as_object())
            .is_some_and(|properties| !properties.is_empty())
        {
            assert_eq!(
                tool.input_schema.get("additionalProperties"),
                Some(&serde_json::Value::Bool(false)),
                "{} must reject unknown input fields",
                tool.name
            );
        }
        assert!(
            tool.output_schema.is_some(),
            "{} must advertise a structured output schema",
            tool.name
        );
    }
}
