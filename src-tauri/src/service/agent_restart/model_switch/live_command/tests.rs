use std::cell::Cell;

use super::*;

fn source() -> ModelSwitchTranscript {
    ModelSwitchTranscript {
        record_type: "opaque-type".into(),
        content_path: vec!["payload".into()],
        prefix: "<opaque>".into(),
        suffix: "</opaque>".into(),
    }
}

#[test]
fn confirmation_records_model_and_effort_without_changing_the_session() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some("owned-session".into());
    tab.session_binding_nonce = Some("owned-nonce".into());
    record_live_request_identity(tab, "chosen-model", Some("chosen-effort"));
    record_confirmed_effort(tab, "chosen-effort");
    assert_eq!(tab.agent_requested_model.as_deref(), Some("chosen-model"));
    assert_eq!(
        tab.agent_provider_resolved_model.as_deref(),
        Some("chosen-model")
    );
    assert_eq!(tab.restorable_launch.model.as_deref(), Some("chosen-model"));
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("chosen-effort")
    );
    assert_eq!(
        tab.restorable_launch.resolved_reasoning_effort.as_deref(),
        Some("chosen-effort")
    );
    assert_eq!(tab.agent_session_id.as_deref(), Some("owned-session"));
    assert_eq!(tab.session_binding_nonce.as_deref(), Some("owned-nonce"));
}

#[test]
fn idle_command_writes_once_and_busy_command_never_writes() {
    for idle in [false, true] {
        let writes = Cell::new(0);
        let result = execute_command(
            || idle,
            || {
                writes.set(writes.get() + 1);
                Ok(())
            },
            || true,
        );
        assert_eq!(writes.get(), usize::from(idle));
        assert_eq!(result.is_ok(), idle);
    }
}

#[test]
fn timeout_fails_without_rewriting_or_restarting() {
    let writes = Cell::new(0);
    let result = execute_command(
        || true,
        || {
            writes.set(writes.get() + 1);
            Ok(())
        },
        || false,
    );
    assert!(result.is_err());
    assert_eq!(writes.get(), 1);
}

#[test]
fn confirmation_requires_the_declared_record_and_exact_wrapped_output() {
    let pattern = confirmation_pattern("^accepted {model}$", "m[1]", "Display", None).unwrap();
    let record = |kind, content| serde_json::json!({"type": kind, "payload": content}).to_string();
    assert!(transcript_confirms(
        &record("opaque-type", "<opaque>accepted m[1]</opaque>"),
        &source(),
        &pattern
    ));
    assert!(!transcript_confirms(
        &record("assistant", "<opaque>accepted m[1]</opaque>"),
        &source(),
        &pattern
    ));
    assert!(!transcript_confirms(
        &record("opaque-type", "accepted m[1]"),
        &source(),
        &pattern
    ));
    assert!(!transcript_confirms(
        &record("opaque-type", "<opaque>accepted m1</opaque>"),
        &source(),
        &pattern
    ));
}

#[test]
fn command_templates_reject_multiline_input_and_unresolved_placeholders() {
    assert_eq!(
        render_command("/choose {model}", "opaque-model", None).unwrap(),
        "/choose opaque-model"
    );
    assert_eq!(
        render_command("/reason {effort}", "opaque-model", Some("opaque-effort")).unwrap(),
        "/reason opaque-effort"
    );
    assert!(render_command("/choose {model}", "model\n/exit", None).is_err());
    assert!(render_command("/choose {typo}", "model", None).is_err());
}

#[test]
fn transcript_boundary_excludes_old_confirmation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned.jsonl");
    let old = format!(
        "{}\n",
        serde_json::json!({"type":"opaque-type", "payload":"<opaque>accepted chosen</opaque>"})
    );
    std::fs::write(&path, &old).unwrap();
    let pattern = Regex::new("^accepted chosen$").unwrap();
    assert!(!read_confirmation(
        &path,
        old.len() as u64,
        &source(),
        &pattern
    ));
    std::fs::write(&path, format!("{old}{old}")).unwrap();
    assert!(read_confirmation(
        &path,
        old.len() as u64,
        &source(),
        &pattern
    ));
}
