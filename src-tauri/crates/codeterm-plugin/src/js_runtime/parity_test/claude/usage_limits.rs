use super::*;

#[test]
fn claude_usage_limits_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let raw = std::fs::read_to_string(workspace_plugins_root()
        .join("provider-claude/tests/fixtures/usage-normalized.json")).unwrap();
    let expected: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for details in [expected["usage_details"].clone(), serde_json::Value::Null] {
        let mut fixture = expected.clone();
        fixture["usage_details"] = details.clone();
        for parser in [native.usage_parser().unwrap(), js.usage_parser().unwrap()] {
            let snapshot = parser.parse(&fixture.to_string()).unwrap();
            assert_eq!(snapshot.session_pct, Some(17.0));
            assert_eq!(snapshot.weekly_pct, Some(60.0));
            assert_eq!(snapshot.usage_details, if details.is_null() { None } else { Some(details.clone()) });
            assert_eq!(snapshot.billing_mode, codeterm_usage::contract::BillingMode::Subscription);
            assert!(snapshot.limits_reported);
        }
    }
    let mut absent = expected.clone();
    absent.as_object_mut().unwrap().remove("usage_details");
    for parser in [native.usage_parser().unwrap(), js.usage_parser().unwrap()] {
        assert!(parser.parse(&absent.to_string()).unwrap().usage_details.is_none());
    }
}
