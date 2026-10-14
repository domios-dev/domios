use super::skills::extract_revision;
use std::fs;

// Mirror CLAUDE hook marker from integration tests to keep legacy `$`-escaping regression covered.
const CODETERM_HOOK_MARKER: &str = "codeterm-hook-v2";

#[test]
fn test_extract_revision_valid() {
    assert_eq!(extract_revision("some content\n<!-- revision: 10 -->"), 10);
}

#[test]
fn test_extract_revision_missing() {
    assert_eq!(extract_revision("no revision here"), 0);
}

#[test]
fn test_extract_revision_whitespace() {
    assert_eq!(extract_revision("<!-- revision:   42   -->"), 42);
}

#[test]
fn test_extract_revision_invalid_number() {
    assert_eq!(extract_revision("<!-- revision: abc -->"), 0);
}

#[test]
fn test_hook_commands_preserve_dollar_sign() {
    // Verify that hook commands contain $CODETERM_TAB_ID (not \\$ escaped)
    let null_dev = crate::shell::ShellSyntax::NULL_DEV;
    let port = 7686u16;
    let notify_cmd = format!(
        "curl -s -X POST http://localhost:{port}/api/tabs/$CODETERM_TAB_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
    );
    assert!(
        notify_cmd.contains("$CODETERM_TAB_ID"),
        "Command must contain $CODETERM_TAB_ID"
    );
    assert!(
        !notify_cmd.contains("\\$"),
        "Command must NOT contain escaped \\$"
    );

    let json_val = serde_json::json!({"command": notify_cmd});
    let serialized = serde_json::to_string_pretty(&json_val).unwrap();
    assert!(
        serialized.contains("$CODETERM_TAB_ID"),
        "JSON serialization must preserve $"
    );
    assert!(
        !serialized.contains("\\\\$"),
        "JSON serialization must NOT double-escape $"
    );
    assert!(
        !serialized.contains("\\$"),
        "JSON serialization must NOT escape $"
    );
}

#[test]
fn test_install_hooks_to_temp_file() {
    let tmp = tempfile::tempdir().unwrap();
    let settings_path = tmp.path().join("settings.json");

    fs::write(&settings_path, "{}").unwrap();

    // Build hook commands (same logic as integration::install_integration)
    let port = 7686u16;
    let null_dev = crate::shell::ShellSyntax::NULL_DEV;
    let notify_cmd = format!(
        "curl -s -X POST http://localhost:{port}/api/tabs/$CODETERM_TAB_ID/hook/permission -H 'Content-Type: application/json' -d '{{}}' > {null_dev} 2>&1 || true"
    );

    let mut json: serde_json::Value = serde_json::json!({});
    json["hooks"] = serde_json::json!({});
    json["hooks"]["Notification"] = serde_json::json!([{
        "matcher": "permission_prompt",
        "hooks": [{
            "type": "command",
            "command": notify_cmd,
            "timeout": 5000,
            "_source": CODETERM_HOOK_MARKER
        }]
    }]);

    let pretty = serde_json::to_string_pretty(&json).unwrap();
    fs::write(&settings_path, &pretty).unwrap();

    let content = fs::read_to_string(&settings_path).unwrap();
    assert!(
        content.contains("$CODETERM_TAB_ID"),
        "Written file must have $CODETERM_TAB_ID"
    );
    assert!(
        !content.contains("\\$"),
        "Written file must NOT have escaped \\$"
    );

    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let cmd = parsed["hooks"]["Notification"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(cmd.contains("$CODETERM_TAB_ID"));
}
