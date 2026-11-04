use super::*;

#[test]
fn open_and_send_children_receive_host_tab_credential_and_nonce() {
    use crate::host_traits::PluginHostCtx;
    use crate::js_runtime::host_compute::tests::AnalyticsHost;

    let expected = "tab-token|chat-tab|launch-nonce";
    let mut host = AnalyticsHost::default();
    host.process_env = HashMap::from([
        ("CODETERM_API_TOKEN".into(), "tab-token".into()),
        ("CODETERM_TAB_ID".into(), "chat-tab".into()),
        (
            "CODETERM_SESSION_BINDING_NONCE".into(),
            "launch-nonce".into(),
        ),
    ]);
    let host: Arc<dyn PluginHostCtx> = Arc::new(host);
    let vm = vm_with(
        r#"
        let last = "";
        function probe(then) {
            const opts = host.platform() === "windows"
                ? { bin: "powershell", args: ["-NoProfile", "-Command",
                    "& powershell -NoProfile -Command '[Console]::Out.Write(($env:CODETERM_API_TOKEN, $env:CODETERM_TAB_ID, $env:CODETERM_SESSION_BINDING_NONCE -join [char]124))'"] }
                : { bin: "sh", args: ["-c", "sh -c 'printf %s \"$CODETERM_API_TOKEN|$CODETERM_TAB_ID|$CODETERM_SESSION_BINDING_NONCE\"'"] };
            opts.env = { CODETERM_API_TOKEN: "owner-secret", CODETERM_TAB_ID: "foreign", CODETERM_SESSION_BINDING_NONCE: "stale" };
            return host.exec.async(opts, function (out) {
                if (out.code !== 0) throw new Error(JSON.stringify(out));
                return then(out.stdout);
            });
        }
        module.exports.default = {
            openSession() { return probe(function (value) { last = value; return { sessionId: "s1" }; }); },
            sendMessage() { return probe(function (value) { last = value; }); },
            poll() { return [{ id: "1", type: "assistant", content: last }]; }
        };
    "#,
    );
    assert!(vm.set_service(Arc::downgrade(&host)));
    let backend = JsChatBackend::new(vm, "test-chatbackend");
    let session = backend
        .open_session(json!({"tabId":"chat-tab", "bindingNonce":"launch-nonce"}))
        .unwrap();
    assert_eq!(
        backend.poll(&session, None).unwrap().messages[0].content,
        expected
    );
    backend.send_message(&session, "opaque").unwrap();
    assert_eq!(
        backend.poll(&session, None).unwrap().messages[0].content,
        expected
    );
    assert!(super::super::host_exec::current_host_env().is_empty());
}

fn vm_with(src: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source("test-chatbackend", src.to_string()).expect("vm"))
}

#[test]
fn open_session_returns_id() {
    let vm = vm_with(
        r#"module.exports.default = {
            openSession(cfg) { return { sessionId: "s-" + (cfg.model || "x") }; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let sid = b.open_session(json!({ "model": "qwen" })).unwrap();
    assert_eq!(sid, "s-qwen");
}

#[test]
fn open_session_forwards_ctx_tab_id() {
    // A plugin keying its session on tabId must receive the full `{ tabId, config }` envelope, not bare config.
    let vm = vm_with(
        r#"module.exports.default = {
            openSession(ctx) { return { sessionId: ctx.tabId }; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let sid = b
        .open_session(json!({ "tabId": "tab-7", "config": {} }))
        .unwrap();
    assert_eq!(sid, "tab-7");
}

#[test]
fn chatbackend_host_exec_env_includes_tab_identity() {
    let vm = Arc::new(
        PluginVm::from_source(
            "test-chatbackend-env",
            "module.exports.default = {};".into(),
        )
        .unwrap(),
    );
    let env = chatbackend_host_exec_env(
        &vm,
        &json!({
            "tabId": "chat-tab",
            "workspaceId": "ws-1",
            "padDir": "/instance/runtime/agent-pads/chat-tab",
            "config": {}
        }),
    )
    .unwrap();

    assert_eq!(
        env.get("CODETERM_TAB_ID").map(String::as_str),
        Some("chat-tab")
    );
    assert_eq!(
        env.get("CODETERM_WORKSPACE_ID").map(String::as_str),
        Some("ws-1")
    );
    assert_eq!(
        env.get("CODETERM_PAD_DIR").map(String::as_str),
        Some("/instance/runtime/agent-pads/chat-tab")
    );
    assert!(env.contains_key("CODETERM_SOCKET_PATH"));
}

#[test]
fn send_message_host_exec_sees_chatbackend_tab_identity() {
    let vm = vm_with(
        r#"let last = "";
           module.exports.default = {
             openSession(ctx) { return { sessionId: "s1" }; },
             sendMessage(sid, text) {
               const opts = host.platform() === "windows"
                 ? { bin: "powershell", args: ["-NoProfile", "-Command", "[Console]::Out.Write($env:CODETERM_TAB_ID)"] }
                 : { bin: "sh", args: ["-c", "printf %s \"$CODETERM_TAB_ID\""] };
               const out = JSON.parse(host.exec(JSON.stringify(opts)));
               last = out.stdout || "";
             },
             poll() { return [{ id: "1", type: "assistant", content: last }]; }
           }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let sid = b
        .open_session(json!({ "tabId": "chat-tab", "config": {} }))
        .unwrap();
    b.send_message(&sid, "reply").unwrap();

    let msgs = b.poll(&sid, None).unwrap().messages;

    assert_eq!(msgs[0].content, "chat-tab");
}

#[test]
fn open_session_forwards_open_shell_ctx_fields() {
    let vm = vm_with(
        r#"module.exports.default = {
            openSession(ctx) {
                return {
                    sessionId: [
                        ctx.tabId,
                        ctx.systemPrompt,
                        ctx.preset,
                        ctx.model
                    ].join("|")
                };
            }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let sid = b
        .open_session(json!({
            "tabId": "tab-7",
            "config": {},
            "systemPrompt": "You are CodeTerm.",
            "preset": "codeterm",
            "model": "qwen"
        }))
        .unwrap();
    assert_eq!(sid, "tab-7|You are CodeTerm.|codeterm|qwen");
}

#[test]
fn open_session_error_propagates() {
    let vm =
        vm_with(r#"module.exports.default = { openSession() { return { error: "no server" }; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(matches!(
        b.open_session(json!({})),
        Err(ChatBackendError::Call(_))
    ));
}

#[test]
fn poll_maps_normalized_messages_type_to_kind() {
    // Plugin returns `{ id, type, content }`; serde maps `type` → ChatMsg.kind.
    let vm = vm_with(
        r#"module.exports.default = {
            poll(sid) { return [
                { id: "1", type: "user", content: "hi" },
                { id: "2", type: "assistant", content: "yo" }
            ]; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let msgs = b.poll("s1", None).unwrap().messages;
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].kind, "user");
    assert_eq!(msgs[0].content, "hi");
    assert_eq!(msgs[1].kind, "assistant");
}

#[test]
fn plugin_icon_svg_poll_replaces_plugin_supplied_icon_with_host_asset() {
    let vm = vm_with(
        r#"module.exports.default = {
            poll() { return [{
                id: "1", type: "user", content: "hello",
                inboundKind: "plugin", fromPlugin: "other",
                pluginIconSvg: "<svg><script>alert(1)</script></svg>"
            }]; }
        }"#,
    );
    let mut b = JsChatBackend::new(vm, "pebble");
    b.icon_svg =
        Some("<svg viewBox=\"0 0 1 1\" fill=\"currentColor\"><path d=\"M0 0\"/></svg>".into());

    let mut messages = b.poll("s1", None).unwrap().messages;
    let message = messages.remove(0);

    assert_eq!(message.inbound_kind.as_deref(), Some("plugin"));
    assert_eq!(message.from_plugin.as_deref(), Some("pebble"));
    assert_eq!(message.plugin_icon_svg.as_deref(), b.icon_svg.as_deref(),);
}

#[test]
fn poll_accepts_wrapped_messages_shape() {
    let vm = vm_with(
        r#"module.exports.default = {
            poll() { return { messages: [{ id: "1", type: "assistant", content: "ok" }] }; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let msgs = b.poll("s1", None).unwrap().messages;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "ok");
}

#[test]
fn poll_forwards_cursor_and_returns_delta_plus_next_cursor() {
    // Contract: poll(sessionId, cursor) -> { messages, cursor } returns only the delta after the cursor, not the whole transcript every tick.
    let vm = vm_with(
        r#"module.exports.default = {
            poll(sid, cursor) {
                if (cursor === "c1") { return { messages: [], cursor: "c1" }; }
                return {
                    messages: [{ id: "1", type: "assistant", content: cursor || "fresh" }],
                    cursor: "c1"
                };
            }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");

    // First poll: no cursor ⇒ full delta + seeded cursor.
    let first = b.poll("s1", None).unwrap();
    assert_eq!(first.messages.len(), 1);
    assert_eq!(first.messages[0].content, "fresh");
    assert_eq!(first.cursor.as_deref(), Some("c1"));

    // Next poll passes that cursor ⇒ empty delta (nothing new), cursor held.
    let second = b.poll("s1", Some("c1")).unwrap();
    assert!(second.messages.is_empty(), "idle poll yields no delta");
    assert_eq!(second.cursor.as_deref(), Some("c1"));
}

#[test]
fn poll_bare_list_has_no_cursor() {
    // cursor None (the loop keeps its prior cursor).
    let vm = vm_with(
        r#"module.exports.default = {
            poll(sid, cursor) { return [{ id: "1", type: "assistant", content: "x" }]; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let out = b.poll("s1", None).unwrap();
    assert_eq!(out.messages.len(), 1);
    assert_eq!(out.cursor, None);
}

#[test]
fn list_models_maps_camel_display_name() {
    let vm = vm_with(
        r#"module.exports.default = {
            listModels() { return [
                { id: "qwen", displayName: "Qwen 2.5", description: "local", available: false },
                { id: "bare" }
            ]; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let models = b.list_models().unwrap();
    assert_eq!(models[0].id, "qwen");
    assert_eq!(models[0].display_name, "Qwen 2.5");
    assert_eq!(models[0].description.as_deref(), Some("local"));
    assert_eq!(models[0].available, Some(false));
    // Missing displayName falls back to the id.
    assert_eq!(models[1].display_name, "bare");
}

#[test]
fn list_models_missing_export_returns_empty_list() {
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(b.list_models().unwrap().is_empty());
}

#[test]
fn list_presets_maps_optional_export() {
    let vm = vm_with(
        r#"module.exports.default = {
            listPresets() { return [
                { id: "codeterm", name: "CodeTerm", description: "default shell" },
                { id: "rhymes", name: "Rhymes" }
            ]; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let presets = b.list_presets().unwrap();
    assert_eq!(presets[0].id, "codeterm");
    assert_eq!(presets[0].name, "CodeTerm");
    assert_eq!(presets[0].description.as_deref(), Some("default shell"));
    assert_eq!(presets[1].id, "rhymes");
    assert_eq!(presets[1].name, "Rhymes");
    assert_eq!(presets[1].description, None);
}

#[test]
fn list_presets_missing_export_returns_empty_list() {
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(b.list_presets().unwrap().is_empty());
}

#[test]
fn send_message_dispatches_without_error() {
    let vm = vm_with(
        r#"module.exports.default = {
            sendMessage(sid, text) { globalThis.__last = sid + ":" + text; }
        }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(b.send_message("s1", "hello").is_ok());
}

#[test]
fn watcher_tick_forwards_session_and_input() {
    let vm = vm_with(
        r#"let last = "";
           module.exports.default = {
             watcherTick(sid, input) { last = JSON.stringify({ sid, input }); },
             poll() { return [{ id: "1", type: "assistant", content: last }]; }
           }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let input = json!({
        "tick": 3,
        "nowMs": 123456,
        "state": { "round": 2 },
        "observations": { "tabs": ["orch", "worker"] }
    });

    b.watcher_tick("watch-1", input.clone()).unwrap();
    let msgs = b.poll("watch-1", None).unwrap().messages;
    let forwarded: serde_json::Value = serde_json::from_str(&msgs[0].content).unwrap();

    assert_eq!(forwarded, json!({ "sid": "watch-1", "input": input }));
}

#[test]
fn watcher_tick_missing_export_errors_unsupported() {
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    let err = b.watcher_tick("s1", json!({ "tick": 1 })).unwrap_err();

    assert!(err
        .to_string()
        .contains("this chatBackend does not support watcher mode"));
}

#[test]
fn send_turn_forwards_session_and_composed_turn() {
    let vm = vm_with(
        r#"let last = "";
           module.exports.default = {
             sendTurn(sid, turn) { last = JSON.stringify({ sid, turn }); },
             poll() { return [{ id: "1", type: "assistant", content: last }]; }
           }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    let turn = HostTurn {
        system: "role brief".into(),
        groups: vec![crate::chatbackend_provider::TurnGroup {
            entries: vec![ChatMsg {
                id: "entry-1".into(),
                kind: "user".into(),
                content: "hello".into(),
                ..ChatMsg::default()
            }],
        }],
    };

    b.send_turn("turn-1", turn.clone()).unwrap();
    let msgs = b.poll("turn-1", None).unwrap().messages;
    let forwarded: serde_json::Value = serde_json::from_str(&msgs[0].content).unwrap();

    assert_eq!(
        forwarded,
        json!({ "sid": "turn-1", "turn": serde_json::to_value(&turn).unwrap() }),
        "the adapter must hand the composed turn over unmodified"
    );
}

#[test]
fn send_turn_missing_export_errors_unsupported() {
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    let err = b.send_turn("s1", HostTurn::default()).unwrap_err();

    assert!(err.to_string().contains(CONTEXT_OWNER_UNSUPPORTED));
}

#[test]
fn context_owner_comes_from_manifest_key() {
    let manifest: PluginManifest = serde_json::from_str(
        r#"{
            "id": "context-owner-fixture",
            "version": "0.1.0",
            "displayName": "Context Owner Fixture",
            "capabilities": { "chatBackend": { "contextOwner": "host" } }
        }"#,
    )
    .unwrap();
    let b = JsChatBackend::from_manifest(vm_with("module.exports.default = {}"), &manifest);

    assert_eq!(b.context_owner(), ContextOwner::Host);
    assert_eq!(b.describe().context_owner, ContextOwner::Host);
}

#[test]
fn context_owner_absent_manifest_key_is_plugin() {
    let manifest: PluginManifest = serde_json::from_str(
        r#"{
            "id": "context-owner-absent-fixture",
            "version": "0.1.0",
            "displayName": "Context Owner Absent Fixture",
            "capabilities": { "chatBackend": { "spawnable": true } }
        }"#,
    )
    .unwrap();
    let b = JsChatBackend::from_manifest(vm_with("module.exports.default = {}"), &manifest);

    assert_eq!(b.context_owner(), ContextOwner::Plugin);
}

#[test]
fn watcher_capable_comes_from_manifest_flag() {
    let manifest: PluginManifest = serde_json::from_str(
        r#"{
            "id": "lmstudio",
            "version": "0.1.0",
            "displayName": "LM Studio",
            "capabilities": { "chatBackend": { "spawnable": true, "watcher": true } }
        }"#,
    )
    .unwrap();
    let b = JsChatBackend::from_manifest(vm_with("module.exports.default = {}"), &manifest);

    assert!(b.spawnable());
    assert!(b.watcher_capable());
}

#[test]
fn pump_advances_state_that_poll_then_surfaces() {
    // pump() runs each tick before poll(); an async plugin advances work in pump, poll surfaces it.
    let vm = vm_with(
        r#"let pumped = 0;
           module.exports.default = {
             pump(sid) { pumped++; },
             poll(sid) { return pumped > 0
                 ? [{ id: "1", type: "assistant", content: "advanced" }]
                 : []; }
           }"#,
    );
    let b = JsChatBackend::new(vm, "npc");
    assert!(
        b.poll("s", None).unwrap().messages.is_empty(),
        "no messages before pump"
    );
    b.pump("s").unwrap();
    let msgs = b.poll("s", None).unwrap().messages;
    assert_eq!(msgs.len(), 1, "pump advanced state poll surfaces");
    assert_eq!(msgs[0].content, "advanced");
}

#[test]
fn pump_missing_export_is_ok() {
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "npc");
    assert!(b.pump("s").is_ok(), "a plugin without pump is unaffected");
}

#[test]
fn close_session_missing_export_is_ok() {
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(b.close_session("s1").is_ok());
}

#[test]
fn set_model_then_session_info_round_trips_via_exports() {
    // switch_model routes to `setModel`; session_info reads the live model back via `sessionInfo`.
    let vm = vm_with(
        r#"let model = "init";
           module.exports.default = {
             setModel(sid, m) { model = m; },
             sessionInfo(sid) { return { model }; }
           }"#,
    );
    let b = JsChatBackend::new(vm, "local-shell");
    assert_eq!(b.session_info("s").unwrap().model.as_deref(), Some("init"));
    b.set_model("s", "qwen-2.5").unwrap();
    assert_eq!(
        b.session_info("s").unwrap().model.as_deref(),
        Some("qwen-2.5"),
        "setModel must update the model session_info surfaces"
    );
}

#[test]
fn session_info_missing_export_is_empty() {
    // A bare backend with no `sessionInfo` export reports no model, never an error.
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(b.session_info("s").unwrap().model.is_none());
}

#[test]
fn set_model_missing_export_errors() {
    // A backend that cannot switch surfaces an error (command reports "unsupported").
    let vm = vm_with(r#"module.exports.default = { poll() { return []; } }"#);
    let b = JsChatBackend::new(vm, "local-shell");
    assert!(b.set_model("s", "m").is_err());
}

#[test]
fn host_environment_failure_refuses_session_before_plugin_code_runs() {
    use crate::host_traits::PluginHostCtx;
    use crate::js_runtime::host_compute::tests::AnalyticsHost;
    let mut host = AnalyticsHost::default();
    host.process_env_error = Some("storage unavailable".into());
    let host: Arc<dyn PluginHostCtx> = Arc::new(host);
    assert_eq!(
        host.tab_process_env("tab", "nonce"),
        Err("storage unavailable".into())
    );
    let vm = vm_with(
        "module.exports.default = { openSession() { throw new Error('plugin must not run'); } };",
    );
    assert!(vm.set_service(Arc::downgrade(&host)));
    let backend = JsChatBackend::new(vm, "test-chatbackend");
    let error = backend
        .open_session(json!({"tabId":"tab","bindingNonce":"nonce"}))
        .unwrap_err();
    assert!(matches!(error, ChatBackendError::Call(ref reason) if reason == "storage unavailable"));
}
