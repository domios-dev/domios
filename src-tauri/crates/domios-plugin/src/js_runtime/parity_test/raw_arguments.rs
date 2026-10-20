use super::load_inline_js;
use crate::js_runtime::vm::{call_json, call_json_strict};
use serde_json::json;

#[test]
fn stored_history_raw_string_matches_native_and_json_channels() {
    let js = load_inline_js(
        "raw-arguments",
        r#"module.exports = {
            runtimeStoredHistory(session, records) {
                if (session === "throw") throw new Error("fixture rejection");
                return [session, records];
            },
            echo(session, records) { return [session, records]; },
            hasRawArgument() { return typeof globalThis.__ct_raw_arg__ === 'string'; }
        };"#,
    );
    for records in [
        "",
        "quotes: \"\nbackslash: \\\r\nNUL: \0 Unicode: 🦀",
        r#"{"high":"\ud800","low":"\udfff","pair":"\ud83e\udd80"}"#,
    ] {
        let session = "session\"\n\\🦀";
        let native = vec![session.to_string(), records.to_string()];
        let args = [json!(session), json!(records)];
        let json: Vec<String> = call_json(js.vm(), "echo", &args).unwrap();
        let raw: Vec<String> = call_json(js.vm(), "runtimeStoredHistory", &args).unwrap();
        let strict: Vec<String> = call_json_strict(js.vm(), "runtimeStoredHistory", &args).unwrap();
        assert_eq!(raw, native);
        assert_eq!(json, native);
        assert_eq!(strict, native);
        assert_eq!(raw[1].as_bytes(), records.as_bytes());
        let next: Vec<String> = call_json(js.vm(), "echo", &args).unwrap();
        assert_eq!(next, native);
    }
    let args = [json!("throw"), json!("large string")];
    assert!(call_json::<Vec<String>>(js.vm(), "runtimeStoredHistory", &args).is_err());
    assert!(!call_json::<bool>(js.vm(), "hasRawArgument", &[]).unwrap());
}
