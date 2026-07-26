use super::*;

#[test]
fn send_text_body_only_text() {
    let json = r#"{"text": "ls"}"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.text, Some("ls".to_string()));
    assert!(body.batch.is_none());
}

/// The plain text send always submits — the `enter` knob was removed. A legacy
/// client that still sends `enter` must keep parsing (the field is ignored).
#[test]
fn send_text_body_ignores_legacy_enter_field() {
    let json = r#"{"text": "partial", "enter": false}"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.text.as_deref(), Some("partial"));
}

#[test]
fn send_text_body_empty_object() {
    let json = r#"{}"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.text, None, "text is None when omitted");
}

#[test]
fn send_text_body_text_null() {
    let json = r#"{"text": null}"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.text, None);
}

#[test]
fn send_text_body_with_batch() {
    let json = r#"{
            "batch": [
                {"text": "cmd1"},
                {"text": "cmd2", "enter": false},
                {"text": "cmd3", "enter": true, "delay_ms": 500}
            ]
        }"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    let batch = body.batch.unwrap();
    assert_eq!(batch.len(), 3);
    assert!(batch[0].enter, "batch item enter defaults to true");
    assert!(!batch[1].enter);
    assert!(batch[2].enter);
    assert_eq!(batch[2].delay_ms, Some(500));
}

#[test]
fn batch_command_enter_defaults_true() {
    let json = r#"{"text": "echo hi"}"#;
    let cmd: BatchCommand = serde_json::from_str(json).unwrap();
    assert!(
        cmd.enter,
        "BatchCommand.enter defaults to true via default_true()"
    );
}

#[test]
fn send_text_body_with_actions() {
    let json = r#"{
            "actions": [
                {"type": "key", "key": "esc"},
                {"type": "delay", "ms": 200},
                {"type": "key", "key": "enter"}
            ]
        }"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    assert_eq!(
        body.actions.unwrap(),
        vec![
            SendAction::Key { key: "esc".into() },
            SendAction::Delay { ms: 200 },
            SendAction::Key {
                key: "enter".into()
            },
        ]
    );
}

#[test]
fn send_action_bytes_cover_navigation_keys() {
    assert_eq!(send_action_bytes("enter").unwrap(), b"\r".to_vec());
    assert_eq!(send_action_bytes("esc").unwrap(), b"\x1b".to_vec());
    assert_eq!(send_action_bytes("up").unwrap(), b"\x1b[A".to_vec());
    assert_eq!(send_action_bytes("ctrl-c").unwrap(), vec![3]);
}

#[test]
fn exec_body_deserialize() {
    let json = r#"{"command": "ls -la"}"#;
    let body: ExecBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.command, "ls -la");
    assert!(body.timeout_ms.is_none());
    assert!(body.strip_ansi.is_none());
}

#[test]
fn exec_body_with_options() {
    let json = r#"{"command": "echo test", "timeout_ms": 5000, "strip_ansi": true}"#;
    let body: ExecBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.timeout_ms, Some(5000));
    assert_eq!(body.strip_ansi, Some(true));
}

fn apply_exec_enter(command: &str) -> String {
    if command.ends_with('\r') || command.ends_with('\n') {
        command.to_string()
    } else {
        format!("{}\r", command)
    }
}

#[test]
fn exec_command_no_trailing_newline_appends_cr() {
    assert_eq!(apply_exec_enter("ls -la"), "ls -la\r");
}

#[test]
fn exec_command_trailing_cr_no_append() {
    assert_eq!(apply_exec_enter("ls\r"), "ls\r");
}

#[test]
fn exec_command_trailing_newline_no_append() {
    assert_eq!(apply_exec_enter("ls\n"), "ls\n");
}

#[test]
fn exec_command_trailing_crlf_no_append() {
    assert_eq!(apply_exec_enter("ls\r\n"), "ls\r\n");
}

#[test]
fn exec_empty_command_appends_cr() {
    assert_eq!(apply_exec_enter(""), "\r");
}

#[test]
fn default_true_returns_true() {
    assert!(default_true());
}

/// send is always fire-and-forget — the `verify` field was removed in #101
/// because TUI agents intercept input and don't echo per-character, making
/// verification unreliable (especially over mesh). Deserialization must
/// accept legacy clients that still send the field by ignoring it.
#[test]
fn send_text_body_ignores_legacy_verify_field() {
    let json = r#"{"text":"hi","verify":true}"#;
    let body: SendTextBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.text.as_deref(), Some("hi"));
}

// --- return-address marker gating: shell runs raw bytes, agent keeps the marker ---

fn render_send(
    has_source: bool,
    target_has_agent: bool,
    transport: PaneInputTransport,
    body: &str,
) -> String {
    if should_prepend_source(has_source, target_has_agent, transport) {
        let envelope =
            crate::service::messenger::MessageEnvelope::pane("Canvas Orchestrator", "073f64ae")
                .model(Some("opus".into()));
        format!(
            "{}{}",
            crate::service::messenger::Messenger::pty_prefix(&envelope),
            body
        )
    } else {
        body.to_string()
    }
}

#[test]
fn send_to_shell_pane_with_source_has_no_marker() {
    // The production bug: source_pane set, target is a bare shell (no agent).
    let out = render_send(
        true,
        false,
        PaneInputTransport::Pty,
        "cd /path && ./build.sh dev",
    );
    assert_eq!(out, "cd /path && ./build.sh dev");
    assert!(
        !out.contains("-=-codeterm:tab"),
        "a bare shell must run raw text, never the return-address marker"
    );
}

#[test]
fn send_to_agent_pane_with_source_keeps_marker() {
    let out = render_send(true, true, PaneInputTransport::Pty, "ship it");
    assert!(
        out.contains("-=-codeterm:tab"),
        "an agent pane still gets the return-address marker"
    );
    assert!(out.ends_with("ship it"));
}

#[test]
fn send_to_chatbackend_with_source_skips_pty_marker() {
    // chatBackend identity is structural — the PTY prefix path is skipped.
    assert!(!should_prepend_source(
        true,
        false,
        PaneInputTransport::ChatBackend
    ));
    assert!(!should_prepend_source(
        true,
        true,
        PaneInputTransport::ChatBackend
    ));
}

#[test]
fn send_without_source_never_marks() {
    assert!(!should_prepend_source(false, true, PaneInputTransport::Pty));
    assert_eq!(
        render_send(false, true, PaneInputTransport::Pty, "ls"),
        "ls"
    );
}

#[test]
fn create_pane_body_parses_chat_backend_and_config() {
    // `POST /api/panes`. chat_backend + config map to PaneCommand::ChatBackend.
    let json = r#"{"chat_backend": "local-shell", "config": {"model": "llama"}}"#;
    let body: CreateShellPaneBody = serde_json::from_str(json).unwrap();
    assert_eq!(body.chat_backend.as_deref(), Some("local-shell"));
    assert_eq!(body.config.unwrap()["model"], "llama");
}

#[test]
fn create_pane_body_defaults_to_shell() {
    let json = r#"{"title": "scratch"}"#;
    let body: CreateShellPaneBody = serde_json::from_str(json).unwrap();
    assert!(body.chat_backend.is_none(), "no chat_backend → shell pane");
    assert!(body.config.is_none());
}
