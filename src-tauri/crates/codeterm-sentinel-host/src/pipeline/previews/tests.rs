use super::*;

#[test]
fn port_previews_bound_oversized_argument_metadata_with_explicit_truncation() {
    let inputs = (0..400)
        .map(|n| {
            (
                format!("{n}{}", "p".repeat(100)),
                preview("in", &Value::Null),
            )
        })
        .collect();
    let evidence = evidence(inputs, None);
    assert!(serde_json::to_vec(&evidence).unwrap().len() <= MAX_JOURNAL_EVIDENCE_BYTES);
    assert_eq!(evidence["ports"]["truncated"], true);
    assert!(evidence["ports"]["inputs"].as_object().unwrap().len() < 400);
}

#[test]
fn port_previews_bound_utf8_values_and_total_serialized_evidence() {
    let value = json!("λ".repeat(5000));
    let port = preview("in", &value);
    assert!(port["preview"].as_str().unwrap().len() <= MAX_VALUE_BYTES);
    assert_eq!(port["bytes"], value.to_string().len());
    assert_eq!(port["truncated"], true);
    assert_eq!(port["type"], "string");
    let inputs = (0..32)
        .map(|n| (format!("port{n}"), preview("in", &value)))
        .collect();
    let evidence = evidence(inputs, Some(&ExecutionEnvelope::from_text("42")));
    assert!(serde_json::to_vec(&evidence).unwrap().len() <= MAX_JOURNAL_EVIDENCE_BYTES);
    assert_eq!(evidence["ports"]["inputs"].as_object().unwrap().len(), 32);
    assert_eq!(evidence["ports"]["outputs"]["out"]["preview"], "42");
    assert!(evidence["ports"]["inputs"]
        .as_object()
        .unwrap()
        .values()
        .all(|v| v["truncated"] == true));
}

#[test]
fn port_previews_redact_before_truncating_and_keep_json_types() {
    for value in [
        json!({"password":"opaque-canary"}),
        json!({"nested":[{"key":"opaque-canary"}]}),
        json!(format!("{} password=opaque-canary", "x".repeat(5000))),
        json!("https://test.invalid/path?token=opaque-canary"),
        json!("AKIA1234567890ABCDEF"),
        json!("github_pat_abcdefghijklmnopqrstuvwxyz"),
    ] {
        let port = preview("in", &value);
        assert_eq!(port["redacted"], true);
        assert_eq!(port["preview"], "[redacted]");
        assert_eq!(port["truncated"], false);
    }
    assert_eq!(
        preview("access_token", &json!("opaque-canary"))["redacted"],
        true
    );
    for (value, kind) in [
        (Value::Null, "null"),
        (json!(false), "boolean"),
        (json!(2), "number"),
        (json!([]), "array"),
        (json!({"n":2}), "object"),
    ] {
        assert_eq!(preview("out", &value)["type"], kind);
        assert_eq!(preview("out", &value)["redacted"], false);
    }
}
