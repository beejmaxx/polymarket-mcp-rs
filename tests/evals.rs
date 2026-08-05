use std::collections::BTreeSet;

use polymarket_mcp_rs::{App, PolymarketServer, ToolProfile};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct EvalSuite {
    schema_version: u32,
    profile: String,
    cases: Vec<EvalCase>,
}

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    category: String,
    prompt: String,
    expected_tools: Vec<String>,
    forbidden_tools: Vec<String>,
}

#[test]
fn chatgpt_eval_fixture_matches_the_exposed_contract() {
    let suite: EvalSuite =
        serde_json::from_str(include_str!("../evals/chatgpt-cases.json")).unwrap();
    assert_eq!(suite.schema_version, 1);
    assert_eq!(suite.profile, "chatgpt");
    assert!(suite.cases.len() >= 12);

    let exposed =
        PolymarketServer::with_profile(App::new_ephemeral().unwrap(), ToolProfile::Chatgpt)
            .tools()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<BTreeSet<_>>();
    let all = PolymarketServer::with_profile(App::new_ephemeral().unwrap(), ToolProfile::All)
        .tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();

    let mut ids = BTreeSet::new();
    let mut categories = BTreeSet::new();
    for case in &suite.cases {
        assert!(ids.insert(&case.id), "duplicate eval ID: {}", case.id);
        assert!(!case.prompt.trim().is_empty(), "empty prompt: {}", case.id);
        categories.insert(case.category.as_str());
        for tool in &case.expected_tools {
            assert!(
                exposed.contains(tool),
                "{} expects a tool outside the ChatGPT profile: {tool}",
                case.id
            );
        }
        for tool in &case.forbidden_tools {
            assert!(
                all.contains(tool),
                "{} names an unknown forbidden tool: {tool}",
                case.id
            );
        }
    }
    assert!(categories.contains("direct"));
    assert!(categories.contains("followup"));
    assert!(categories.contains("negative"));
}
