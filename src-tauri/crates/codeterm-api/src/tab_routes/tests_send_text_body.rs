use super::*;

// --- return-address marker gating: shell runs raw bytes, agent keeps the marker ---

fn render_send(
    has_source: bool,
    target_has_agent: bool,
    transport: TabInputTransport,
    body: &str,
) -> String {
    if should_prepend_source(has_source, target_has_agent, transport) {
        let envelope = codeterm_service_core::service::messenger::MessageEnvelope::tab(
            "Canvas Orchestrator",
            "073f64ae",
        )
        .model(Some("opus".into()));
        codeterm_service_core::service::messenger::Messenger::from(envelope).wrap(body)
    } else {
        body.to_string()
    }
}

#[test]
fn send_to_shell_tab_with_source_has_no_marker() {
    // The production bug: source_tab set, target is a bare shell (no agent).
    let out = render_send(
        true,
        false,
        TabInputTransport::Pty,
        "cd /path && ./build.sh dev",
    );
    assert_eq!(out, "cd /path && ./build.sh dev");
    assert!(
        !out.contains("<domios from=\"tab\""),
        "a bare shell must run raw text, never the return-address tag"
    );
}

#[test]
fn send_to_agent_tab_with_source_keeps_marker() {
    let out = render_send(true, true, TabInputTransport::Pty, "ship it");
    assert!(
        out.starts_with("<domios from=\"tab\""),
        "an agent Tab still gets the return-address tag"
    );
    let frame = codeterm_wire::domios::parse_message(&out).expect("complete route frame parses");
    assert_eq!(frame.body, "ship it");
    assert!(frame.content.starts_with("ship it "));
}

#[test]
fn send_to_chatbackend_with_source_skips_pty_marker() {
    // chatBackend identity is structural — the PTY prefix path is skipped.
    assert!(!should_prepend_source(
        true,
        false,
        TabInputTransport::ChatBackend
    ));
    assert!(!should_prepend_source(
        true,
        true,
        TabInputTransport::ChatBackend
    ));
}

#[test]
fn send_without_source_never_marks() {
    assert!(!should_prepend_source(false, true, TabInputTransport::Pty));
    assert_eq!(render_send(false, true, TabInputTransport::Pty, "ls"), "ls");
}
