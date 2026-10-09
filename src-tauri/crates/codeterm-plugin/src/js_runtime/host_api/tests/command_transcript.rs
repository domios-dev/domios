use super::*;

pub(super) fn verify() {
    let vm = vm_with("module.exports.default = { parse(record, declaration) { return host.commandTranscriptStdout(record, declaration); } };");
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/plugin.json"
    )))
    .unwrap();
    let source: crate::provider::ModelSwitchTranscript =
        serde_json::from_value(manifest["commands"]["modelSwitchCommands"]["transcript"].clone())
            .unwrap();
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/model-switch/command-transcript.json"
    )))
    .unwrap();
    let declaration = serde_json::to_string(&source).unwrap();
    for case in cases {
        let record = &case["record"];
        let expected: Option<String> = serde_json::from_value(case["stdout"].clone()).unwrap();
        let actual: Option<String> = crate::js_runtime::vm::call_json(
            &vm,
            "parse",
            &[json!(record.to_string()), json!(declaration)],
        )
        .unwrap();
        assert_eq!(source.record_stdout(record), expected);
        assert_eq!(actual, expected);
        assert_eq!(
            source.has_foreign_user_turn(&record.to_string()).unwrap(),
            case["foreign"].as_bool().unwrap()
        );
    }
    for record in ["invalid".to_owned(), "x".repeat(1024 * 1024 + 1)] {
        let actual: Option<String> =
            crate::js_runtime::vm::call_json(&vm, "parse", &[json!(record), json!(declaration)])
                .unwrap();
        assert_eq!(actual, None);
    }
}
