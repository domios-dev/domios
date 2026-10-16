use super::*;

#[test]
fn claude_session_parser_argument_channels_preserve_rows() {
    let vm = PluginVm::from_source(
        "claude-channel-parity",
        include_str!("../../../../../../plugins/provider-claude/plugin.js").into(),
    )
    .unwrap();
    for records in [
        include_str!("../../../../../../plugins/provider-claude/tests/fixtures/file-diffs.jsonl"),
        include_str!("../../../../../../plugins/provider-claude/tests/fixtures/queued-mid-turn.jsonl"),
        include_str!("../../../../../../plugins/provider-claude/tests/fixtures/owner-mid-turn-2.1.296.jsonl"),
        "{\"type\":\"user\",\"message\":{\"content\":\"quotes \\\" slash \\\\ null \\u0000 🦀\"}}\n",
    ] {
        let args = [serde_json::json!(records)];
        for method in ["parseSessionDelta", "parseSessionMessages"] {
            let json = Arguments {
                json: serde_json::to_string(&args).unwrap(),
                raw: None,
            };
            let raw = Arguments::prepare(method, &args).unwrap();
            assert!(raw.raw.is_some());
            let expected = enter_call(&vm, method, &json, false, None, None).unwrap();
            let actual = enter_call(&vm, method, &raw, false, None, None).unwrap();
            assert_eq!(actual, expected, "{method}");
            let rows: serde_json::Value = serde_json::from_str(&actual).unwrap();
            let rows = if method == "parseSessionDelta" {
                &rows["messages"]
            } else {
                &rows
            };
            assert!(rows.is_array());
            if records.contains("quotes") {
                assert!(!rows.as_array().unwrap().is_empty());
            }
        }
    }
}

#[test]
fn stored_history_argument_measurement() {
    let vm = PluginVm::from_source(
        "argument-measurement",
        "module.exports = { runtimeStoredHistory(session, records) { return records.length; } };"
            .into(),
    )
    .unwrap();
    let mut records =
        "{\"text\":\"quotes \\\" and slash \\\\ and escaped surrogate \\ud800\"}\n".repeat(20_000);
    records.truncate(1_100_000);
    let args = [serde_json::json!("session"), serde_json::json!(records)];
    for raw in [false, true] {
        let mut prepare = std::time::Duration::ZERO;
        let mut execute = std::time::Duration::ZERO;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let prepared = if raw {
                Arguments::prepare("runtimeStoredHistory", &args).unwrap()
            } else {
                Arguments {
                    json: serde_json::to_string(&args).unwrap(),
                    raw: None,
                }
            };
            prepare += start.elapsed();
            let start = std::time::Instant::now();
            let output =
                enter_call(&vm, "runtimeStoredHistory", &prepared, false, None, None).unwrap();
            execute += start.elapsed();
            assert_eq!(output.parse::<usize>().unwrap(), records.len());
        }
        eprintln!(
        "argument-channel raw={raw} bytes={} iterations=5 prepare_mean_us={} execute_mean_us={}",
        records.len(), prepare.as_micros() / 5, execute.as_micros() / 5
    );
    }
}
