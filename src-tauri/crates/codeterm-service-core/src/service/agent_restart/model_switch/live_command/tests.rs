use std::cell::Cell;

use super::*;

#[test]
fn captured_switch_outputs_confirm_model_and_effort_from_exact_records() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/plugin.json"
    )))
    .unwrap();
    let commands: crate::agent::provider::ModelSwitchCommands =
        serde_json::from_value(manifest["commands"]["modelSwitchCommands"].clone()).unwrap();
    let records: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/model-switch/live-command-records.json"
    )))
    .unwrap();
    let bytes = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let model =
        confirmation_pattern(&commands.model.confirmation, "opus", "Opus 5.5", None).unwrap();
    let effort = confirmation_pattern(
        &commands.effort.unwrap().confirmation,
        "opus",
        "Opus 5.5",
        Some("low"),
    )
    .unwrap();
    assert!(transcript_confirms(&bytes, &commands.transcript, &model));
    assert!(transcript_confirms(&bytes, &commands.transcript, &effort));
    assert!(!transcript_confirms(
        &records[0].to_string(),
        &commands.transcript,
        &model
    ));
    assert!(!transcript_confirms(
        &records[2].to_string(),
        &commands.transcript,
        &effort
    ));
}

#[test]
fn session_identity_planner_uses_actual_model_after_unconfirmed_picker_switch() {
    use crate::agent::provider::ModelSwitchConfirmationSource::{SessionIdentity, Transcript};
    let current = planning_model(SessionIdentity, Some("previous".into()), || {
        Some("actual".into())
    });
    assert_eq!(current.as_deref(), Some("actual"));
    assert_eq!(
        planning_model(SessionIdentity, Some("previous".into()), || None),
        None
    );
    assert_eq!(
        planning_model(Transcript, Some("previous".into()), || panic!(
            "transcript uses its record"
        )),
        Some("previous".into())
    );
}

fn source() -> ModelSwitchTranscript {
    ModelSwitchTranscript {
        record_type: "opaque-type".into(),
        content_path: vec!["payload".into()],
        prefix: "<opaque>".into(),
        suffix: "</opaque>".into(),
        ignored_content_regexes: Vec::new(),
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
    assert!(result.unwrap_err().contains("restart and resume"));
    assert_eq!(writes.get(), 1);
}

fn target() -> CommandTarget {
    CommandTarget {
        tab_id: "tab".into(),
        pty_id: "pty".into(),
        provider: "provider".into(),
        session_id: "session".into(),
        binding_nonce: "nonce".into(),
        cwd: None,
        model: "m[1]".into(),
        readiness: None,
        display_name: "Display".into(),
    }
}

fn command_prompt() -> CommandPrompt {
    CommandPrompt::new(
        &crate::agent::provider::ModelSwitchPrompt {
            screen_regex: "^confirm {model}$".into(),
            response_keys: vec!["y".into(), "\r".into()],
            cancel_keys: vec!["\u{1b}".into()],
        },
        &target(),
        None,
    )
    .unwrap()
}

#[test]
fn declared_prompt_answers_once_only_after_own_injection() {
    let writes = Cell::new(0);
    let write = |keys: &[u8]| {
        assert_eq!(keys, b"y\r");
        writes.set(writes.get() + 1);
        Ok(())
    };
    let mut prompt = command_prompt();
    prompt.respond("confirm m[1]", write).unwrap();
    assert_eq!(writes.get(), 0);
    execute_command(
        || true,
        || Ok(()),
        || {
            prompt.injected();
            prompt.respond("confirm other", write).unwrap();
            prompt.respond("confirm m[1]", write).unwrap();
            prompt.respond("confirm m[1]", write).unwrap();
            true
        },
    )
    .unwrap();
    assert_eq!(writes.get(), 1);
}

#[test]
fn failed_injection_never_authorizes_prompt_response() {
    let mut prompt = command_prompt();
    assert!(execute_command(
        || true,
        || Err("write refused".into()),
        || {
            prompt.injected();
            true
        }
    )
    .is_err());
    prompt
        .respond("confirm m[1]", |_| panic!("unowned prompt response"))
        .unwrap();
}

#[test]
fn answered_prompt_timeout_fails_without_repeating_response() {
    let writes = Cell::new(0);
    let mut prompt = command_prompt();
    let result = execute_command(
        || true,
        || Ok(()),
        || {
            prompt.injected();
            for _ in 0..3 {
                prompt
                    .respond("confirm m[1]", |_| {
                        writes.set(writes.get() + 1);
                        Ok(())
                    })
                    .unwrap();
            }
            false
        },
    );
    assert!(result.unwrap_err().contains("restart and resume"));
    assert_eq!(writes.get(), 1);
}

#[test]
fn response_failure_is_not_retried() {
    let mut prompt = command_prompt();
    prompt.injected();
    assert!(prompt
        .respond("confirm m[1]", |_| Err("closed".into()))
        .unwrap_err()
        .contains("could not be answered"));
    prompt
        .respond("confirm m[1]", |_| panic!("response replay"))
        .unwrap();
}

#[test]
fn command_target_refuses_rebound_session_pty_and_nonce() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    let expected = target();
    tab.pty_id = Some(expected.pty_id.clone());
    tab.agent_type = Some(expected.provider.clone());
    tab.agent_session_id = Some(expected.session_id.clone());
    tab.session_binding_nonce = Some(expected.binding_nonce.clone());
    tab.reported_session_binding_nonce = Some(expected.binding_nonce.clone());
    assert!(expected.matches(tab));
    tab.pty_id = Some("replacement".into());
    assert!(!expected.matches(tab));
    tab.pty_id = Some(expected.pty_id.clone());
    tab.agent_session_id = Some("replacement".into());
    assert!(!expected.matches(tab));
    tab.agent_session_id = Some(expected.session_id.clone());
    tab.session_binding_nonce = Some("replacement".into());
    tab.reported_session_binding_nonce = Some("replacement".into());
    assert!(!expected.matches(tab));
}

#[test]
fn preexisting_declared_prompt_refuses_command_without_answering() {
    let mut prompt = command_prompt();
    let result = execute_command(
        || !prompt.present("confirm m[1]"),
        || panic!("command injection"),
        || true,
    );
    assert!(result.is_err());
    prompt
        .respond("confirm m[1]", |_| panic!("preexisting prompt response"))
        .unwrap();
}

#[test]
fn invalid_prompt_declarations_refuse_before_injection() {
    for (screen_regex, response_keys) in [
        ("[", vec!["y"]),
        (".*", vec!["y"]),
        ("confirm", vec![]),
        ("confirm", vec![""]),
    ] {
        assert!(CommandPrompt::new(
            &crate::agent::provider::ModelSwitchPrompt {
                screen_regex: screen_regex.into(),
                response_keys: response_keys.into_iter().map(str::to_owned).collect(),
                cancel_keys: vec![],
            },
            &target(),
            None
        )
        .is_err());
    }
}

#[test]
fn effort_only_plan_sends_only_effort_and_preserves_model_identity() {
    let commands = crate::agent::provider::ModelSwitchCommands {
        turn_completion: None,
        ready_input_regex: None,
        confirmation_source: Default::default(),
        model: ModelSwitchCommand {
            confirmation_prompt_kind: None,
            command: "/choose {model}".into(),
            confirmation: "model".into(),
            prompt: None,
            picker: None,
        },
        effort: Some(ModelSwitchCommand {
            confirmation_prompt_kind: None,
            command: "/reason {effort}".into(),
            confirmation: "^accepted {effort}$".into(),
            prompt: None,
            picker: None,
        }),
        transcript: source(),
    };
    let plan = planned_commands(&commands, false, Some("high"), Some("medium"));
    assert_eq!(plan.len(), 1);
    assert_eq!(
        render_command(&plan[0].0.command, "chosen", plan[0].1).unwrap(),
        "/reason high"
    );
    let pattern =
        confirmation_pattern(&plan[0].0.confirmation, "chosen", "Display", plan[0].1).unwrap();
    assert!(transcript_confirms(
        &serde_json::json!({"type":"opaque-type", "payload":"<opaque>accepted high</opaque>"})
            .to_string(),
        &source(),
        &pattern
    ));
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_requested_model = Some("chosen".into());
    tab.agent_session_model = Some("actual-model".into());
    record_confirmed_effort(tab, "high");
    assert_eq!(tab.agent_requested_model.as_deref(), Some("chosen"));
    assert_eq!(tab.agent_session_model.as_deref(), Some("actual-model"));
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
    assert_eq!(
        planned_commands(&commands, true, Some("high"), Some("medium")).len(),
        2
    );
    let plan = planned_commands(&commands, true, Some("medium"), Some("medium"));
    assert_eq!(plan.len(), 1);
    assert_eq!(
        render_command(&plan[0].0.command, "chosen", plan[0].1).unwrap(),
        "/choose chosen"
    );
    assert!(planned_commands(&commands, false, Some("medium"), Some("medium")).is_empty());
    assert!(planned_commands(&commands, false, None, Some("medium")).is_empty());
    assert_eq!(
        planned_commands(&commands, true, Some("medium"), None).len(),
        2
    );
}

#[test]
fn effort_only_confirmation_records_high_with_unchanged_session_pty_and_nonce() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    let mut target = target();
    target.model = "opus".into();
    tab.pty_id = Some(target.pty_id.clone());
    tab.agent_type = Some(target.provider.clone());
    tab.agent_session_id = Some(target.session_id.clone());
    tab.session_binding_nonce = Some(target.binding_nonce.clone());
    tab.reported_session_binding_nonce = Some(target.binding_nonce.clone());
    tab.agent_session_model = Some("opus".into());
    tab.agent_requested_model = Some("opus".into());
    tab.agent_provider_resolved_model = Some("opus".into());
    record_confirmed_effort(tab, "low");
    let before = (
        tab.agent_session_id.clone(),
        tab.pty_id.clone(),
        tab.session_binding_nonce.clone(),
    );
    assert!(tab.agent_session_model_identity.is_none());
    record_target_effort(&target, tab, "high").unwrap();
    assert_eq!(
        tab.reasoning_effort_identity.requested.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.resolved.as_deref(),
        Some("high")
    );
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
    assert_eq!(tab.agent_requested_model.as_deref(), Some("opus"));
    assert_eq!(tab.agent_provider_resolved_model.as_deref(), Some("opus"));
    assert_eq!(tab.agent_session_model.as_deref(), Some("opus"));
    assert!(tab.agent_session_model_identity.is_none());
    assert_eq!(
        (
            tab.agent_session_id.clone(),
            tab.pty_id.clone(),
            tab.session_binding_nonce.clone()
        ),
        before
    );
    tab.session_binding_nonce = Some("new-nonce".into());
    assert!(record_target_effort(&target, tab, "low").is_err());
    assert_eq!(
        tab.reasoning_effort_identity.actual.as_deref(),
        Some("high")
    );
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
fn formatted_command_stdout_confirms_without_waiting_for_an_assistant_turn() {
    let declaration = codeterm_plugin::agent_claude::ClaudeProvider::new();
    let commands =
        crate::agent::provider::AgentProvider::model_switch_commands(&declaration).unwrap();
    let pattern = confirmation_pattern(
        &commands.model.confirmation,
        "sonnet",
        "Claude Sonnet",
        None,
    )
    .unwrap();
    let record = |text: &str| {
        serde_json::json!({"type":"user", "message":{"content":
        format!("<local-command-stdout>{text}</local-command-stdout>")}})
        .to_string()
    };
    for text in [
        "Set model to `Sonnet 5.5` for this session only",
        "Set model to \u{1b}[1mSonnet 5.5\u{1b}[22m for this session only",
    ] {
        assert!(transcript_confirms(
            &record(text),
            &commands.transcript,
            &pattern
        ));
    }
    for text in [
        "Set model to `Haiku 5.5` for this session only",
        "Set model to `Sonnet 5.5` and saved as your default for new sessions",
    ] {
        assert!(!transcript_confirms(
            &record(text),
            &commands.transcript,
            &pattern
        ));
    }
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

#[test]
fn cancellation_never_touches_a_preexisting_or_different_dialog() {
    let mut prompt = command_prompt();
    assert!(prompt
        .cancel("confirm m[1]", |_| panic!("unowned cancellation"))
        .unwrap());
    prompt.injected();
    assert!(prompt
        .cancel("confirm other", |_| panic!("foreign cancellation"))
        .unwrap());
}

#[test]
fn cancellation_is_once_and_requires_observed_clearance() {
    let mut prompt = command_prompt();
    prompt.injected();
    assert!(!prompt
        .cancel("confirm m[1]", |keys| {
            assert_eq!(keys, b"\x1b");
            Ok(())
        })
        .unwrap());
    assert!(!prompt
        .cancel("confirm m[1]", |_| panic!("cancellation replay"))
        .unwrap());
    assert!(prompt
        .cancel("composer", |_| panic!("unexpected write"))
        .unwrap());
}

#[test]
fn cancelled_prompt_never_accepts_a_late_confirmation() {
    let mut prompt = command_prompt();
    prompt.injected();
    assert!(!prompt.cancel("confirm m[1]", |_| Ok(())).unwrap());
    prompt
        .respond("confirm m[1]", |_| {
            panic!("late confirmation after cancellation")
        })
        .unwrap();
}

#[test]
fn missing_cancellation_declaration_reports_the_open_dialog() {
    let mut declaration = crate::agent::provider::ModelSwitchPrompt {
        screen_regex: "^confirm {model}$".into(),
        response_keys: vec!["y".into()],
        cancel_keys: vec![],
    };
    let mut prompt = CommandPrompt::new(&declaration, &target(), None).unwrap();
    prompt.injected();
    assert!(prompt
        .cancel("confirm m[1]", |_| panic!("undeclared cancellation"))
        .is_err());
    declaration.cancel_keys = vec!["".into()];
    assert!(CommandPrompt::new(&declaration, &target(), None).is_err());
    declaration.cancel_keys = vec!["x".repeat(65)];
    assert!(CommandPrompt::new(&declaration, &target(), None).is_err());
}
