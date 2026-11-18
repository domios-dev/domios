use super::*;

#[test]
fn help_aliases_return_catalog_without_executing_plugin_code() {
    let vm = vm_with(
        "module.exports.default = { onAgentCommand() { throw new Error('must not run'); } };",
    );
    let catalog = AgentVerbCatalog::compose(
        "fixture",
        "Fixture",
        None,
        &["lookup <query>".into()],
        None,
        &[],
    )
    .with_guide(Some("opaque-guide-q7".into()));
    let handler = JsAgentCommandHandler::new(vm, "fixture", true).with_catalog(catalog);
    for verb in ["help", "--help", "-h"] {
        let out = handler
            .on_agent_command("session", verb, &[], None)
            .unwrap();
        assert_eq!(
            out.result,
            "domios plugin fixture <verb>\n\nlookup <query>\n\nopaque-guide-q7"
        );
        assert!(!out.catalogue_changed);
    }
}

#[test]
fn read_only_interruption_does_not_change_write_uncertainty() {
    let handler =
        JsAgentCommandHandler::new(vm_with("module.exports.default = {};"), "fixture", true)
            .with_read_only_verbs(vec!["lookup".into()]);
    let read = handler.command_error("lookup", super::super::vm::VmError::IoUnconfirmed);
    assert_eq!(read, "plugin test-agent-commands onAgentCommand lookup failed: read-only command result unavailable after host I/O interruption");
    for verb in ["send", "unknown"] {
        let write = handler.command_error(verb, super::super::vm::VmError::IoUnconfirmed);
        assert!(write.ends_with("plugin I/O may have started; the operation must not be replayed"));
    }
    assert!(handler
        .command_error(
            "lookup",
            super::super::vm::VmError::Script("opaque-error".into())
        )
        .ends_with("plugin script error: opaque-error"));
}

fn vm_with(src: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source("test-agent-commands", src.to_string()).expect("vm"))
}

#[test]
fn continuation_limit_after_exec_keeps_the_specific_error_and_consumes_jobs() {
    let plugin = "command-continuation-limit";
    let job = super::super::host_exec::start_json_job(plugin, || {
        json!({"done":true,"code":0,"stdout":"x".repeat(85_854)}).to_string()
    })
    .unwrap();
    let source = format!(
        r#"module.exports = {{
        onAgentCommand() {{ return host.awaitJob({job:?}, function() {{
            return host.io.run((function*() {{
                yield* host.io.exec({{bin:'domios-test-missing-executable-q7'}});
                for (var i = 0; i < 257; i++) yield* host.io.fs.fileExists('opaque');
                return {{result:'must not reach'}};
            }})());
        }}); }}
    }};"#
    );
    let vm = Arc::new(PluginVm::from_source(plugin, source).unwrap());
    let handler =
        JsAgentCommandHandler::new(vm, plugin, true).with_read_only_verbs(vec!["lookup".into()]);
    let error = handler
        .on_agent_command("session", "lookup", &[], None)
        .unwrap_err();
    assert!(error.contains("continuation_limit:"), "{error}");
    assert!(!error.contains("I/O interruption"), "{error}");
    assert_eq!(super::super::host_exec::plugin_job_count(plugin), 0);
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
fn auth_responses_update_catalog_state_and_refusals_offer_login() {
    use crate::plugin_connection::PluginConnectionState;
    let vm = vm_with(
        r#"module.exports = {
        onAgentCommand(ctx) {
            if (ctx.verb === 'health') return {result: JSON.stringify({state:'logged-in', deviceCode:'private-code'})};
            if (ctx.verb === 'logout') return {result:'Signed out'};
            return {error:'not-installed: helper missing'};
        }
    }"#,
    );
    let catalog = AgentVerbCatalog::compose(
        "opaque-client",
        "Fixture",
        None,
        &["login".into(), "health".into(), "logout".into()],
        None,
        &[],
    );
    let handler = JsAgentCommandHandler::new(vm, "opaque-client", true).with_catalog(catalog);
    handler
        .on_agent_command("tab", "health", &[], None)
        .unwrap();
    assert_eq!(
        handler.catalog().unwrap().connection_state,
        Some(PluginConnectionState::SignedIn)
    );
    handler
        .on_agent_command("tab", "logout", &[], None)
        .unwrap();
    assert_eq!(
        handler.catalog().unwrap().connection_state,
        Some(PluginConnectionState::SignedOut)
    );
    let error = handler
        .on_agent_command("tab", "chats", &[], None)
        .unwrap_err();
    assert!(error.contains("`domios plugin opaque-client login`"));
    assert!(error.contains("sign-in code or QR"));
    assert_eq!(
        handler.catalog().unwrap().connection_state,
        Some(PluginConnectionState::RuntimeMissing)
    );
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

#[test]
fn an_overrunning_command_names_its_plugin_verb_and_call_deadline() {
    let plugin = "agent-command-deadline";
    super::super::host_exec::set_test_wait_bound(plugin, std::time::Duration::from_millis(50));
    let vm = Arc::new(
        PluginVm::from_source(
            plugin,
            "module.exports.default = { onAgentCommand() { while (true) {} } };".into(),
        )
        .unwrap(),
    );
    let error = JsAgentCommandHandler::new(vm, plugin, true)
        .on_agent_command("session", "open", &[], None)
        .unwrap_err();
    assert_eq!(
        error,
        "plugin agent-command-deadline onAgentCommand open exceeded its 50 ms call deadline"
    );
}
