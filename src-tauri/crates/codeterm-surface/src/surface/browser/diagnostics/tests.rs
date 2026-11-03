use super::*;

#[test]
fn browser_diagnostics_bound_and_redact_records() {
    let row = record("Network.requestWillBeSent",&json!({"request":{"url":"https://user:pass@example.test/path?token=opaque#secret","method":"POST","headers":{"Authorization":"secret"},"postData":"secret"}})).unwrap();
    assert_eq!(row["url"], "https://example.test/path");
    assert!(!row.to_string().contains("secret"));
    assert!(record(
        "Network.requestWillBeSent",
        &json!({"request":{"url":"data:text/plain,secret"}})
    )
    .is_none());
    let row = record("Runtime.consoleAPICalled",&json!({"type":"log","args":[{"value":"token=opaque"},{"description":"secret","objectId":"secret"}]})).unwrap();
    assert_eq!(row["text"], "token=REDACTED [object omitted]");
    let row = record(
        "Runtime.consoleAPICalled",
        &json!({"args":[{"value":"x".repeat(10000)}]}),
    )
    .unwrap();
    assert_eq!(row["text"].as_str().unwrap().len(), 2000);
}

#[test]
fn browser_diagnostics_argument_validation() {
    let args = |words: &[&str]| words.iter().map(|s| (*s).into()).collect::<Vec<String>>();
    assert_eq!(
        parse(&args(&["--pattern", "error", "--limit", "20"]), true).unwrap(),
        (Some("error".into()), 20)
    );
    for words in [
        vec!["--limit", "0"],
        vec!["--limit", "201"],
        vec!["--limit"],
        vec!["--limit", "2", "--limit", "3"],
        vec!["--unknown", "x"],
    ] {
        assert!(parse(&args(&words), true).is_err());
    }
    assert!(parse(&args(&["--pattern", "x"]), false).is_err());
}
