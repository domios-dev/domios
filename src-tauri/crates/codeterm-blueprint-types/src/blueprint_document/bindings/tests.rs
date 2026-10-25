use super::*;

#[test]
fn blueprint_binding_parser_shared_fixtures() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/blueprint-bindings.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let binding = case["binding"].as_str().unwrap();
        let parsed = parse(binding).ok();
        assert_eq!(parsed.map(|p| p.node), case["node"].as_str(), "{binding}");
        assert_eq!(
            parsed.map(|p| p.selection),
            case["selection"].as_str(),
            "{binding}"
        );
    }
}
