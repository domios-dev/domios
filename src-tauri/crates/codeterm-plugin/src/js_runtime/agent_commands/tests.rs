use super::*;

fn vm_with(src: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source("test-agent-commands", src.to_string()).expect("vm"))
}

#[test]
fn forwards_ctx_and_returns_result() {
    let vm = vm_with(
        r#"module.exports.default = {
            onAgentCommand(ctx) {
                return { result: ctx.sessionId + "/" + ctx.verb + "/" + ctx.args.join("+") };
            }
        }"#,
    );
    let h = JsAgentCommandHandler::new(vm, "npc", true);
    let out = h
        .on_agent_command("s1", "say", &["hello".into(), "world".into()], None)
        .unwrap();
    assert_eq!(out.result, "s1/say/hello+world");
    assert!(!out.catalogue_changed);
}

#[test]
fn catalogue_changed_flag_is_returned_from_agent_command() {
    let vm = vm_with(
        r#"module.exports.default = {
            onAgentCommand() { return { result: "updated", catalogueChanged: true }; }
        }"#,
    );
    let h = JsAgentCommandHandler::new(vm, "provider-plugin", true);

    let out = h.on_agent_command("s1", "refresh", &[], None).unwrap();

    assert_eq!(out.result, "updated");
    assert!(out.catalogue_changed);
}

#[test]
fn plugin_error_propagates_as_err() {
    let vm = vm_with(
        r#"module.exports.default = { onAgentCommand() { return { error: "bad verb" }; } }"#,
    );
    let h = JsAgentCommandHandler::new(vm, "npc", true);
    assert_eq!(
        h.on_agent_command("s1", "boom", &[], None).unwrap_err(),
        "bad verb"
    );
}

#[test]
fn missing_export_is_err() {
    let vm = vm_with(r#"module.exports.default = {}"#);
    let h = JsAgentCommandHandler::new(vm, "npc", true);
    assert!(h.on_agent_command("s1", "say", &[], None).is_err());
}

#[test]
fn granted_flag_is_carried() {
    let vm = vm_with(r#"module.exports.default = { onAgentCommand() { return { result: "" }; } }"#);
    assert!(!JsAgentCommandHandler::new(vm.clone(), "npc", false).granted());
    assert!(JsAgentCommandHandler::new(vm, "npc", true).granted());
}

#[test]
fn body_reaches_quickjs_exactly_and_absent_body_stays_optional() {
    let vm = vm_with(
        r#"module.exports.default = {
        onAgentCommand(ctx) { return { result: JSON.stringify(ctx) }; }
    }"#,
    );
    let handler = JsAgentCommandHandler::new(vm, "npc", true);
    for body in [
        None,
        Some(""),
        Some("--message"),
        Some("--"),
        Some("quotes \" $value `\r\nλ😀\n"),
    ] {
        let result = handler
            .on_agent_command("session", "send", &["--".into(), "recipient".into()], body)
            .unwrap();
        let ctx: serde_json::Value = serde_json::from_str(&result.result).unwrap();
        let mut expected =
            json!({"sessionId": "session", "verb": "send", "args": ["--", "recipient"]});
        if let Some(body) = body {
            expected["body"] = body.into();
        }
        assert_eq!(ctx, expected);
    }
}
