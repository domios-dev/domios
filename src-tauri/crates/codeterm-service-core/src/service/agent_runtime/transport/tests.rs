use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};

#[test]
fn neighbor_reply_guidance_uses_the_bound_transport_for_each_access_level() {
    use crate::service::messenger::{AgentInput, InputProvenance, Messenger};
    use codeterm_plugin::agent_runtime::*;

    let friendship = "a".repeat(64);
    let question = "b".repeat(32);
    let stale = "opaque previous reply hint";
    let current = "C:/current-server/codeterm.exe";
    let retained = format!("<domios from=\"neighbor\" friendship=\"{friendship}\" message=\"{question}\" kind=\"question\">opaque body &lt;tip&gt; <tip>{stale}</tip></domios>");
    let restore = || {
        AgentInput::Framed(
            Messenger::restore_framed_delivery(&retained)
                .unwrap()
                .with_provenance(InputProvenance::Neighbor),
        )
    };
    let mut restored = RuntimeContext {
        tab_id: "own".into(),
        binding_nonce: "new-nonce".into(),
        epoch: 4,
        cwd: "C:/friend".into(),
        thread_id: Some("restored".into()),
        ..Default::default()
    };
    let policy = RestrictedCliPolicy {
        tab_id: restored.tab_id.clone(),
        binding_nonce: restored.binding_nonce.clone(),
        epoch: restored.epoch,
        cwd: restored.cwd.clone(),
        metadata_directory: ".fixture".into(),
        executable: current.into(),
        executable_sha256: "a".repeat(64),
        config_sha256: "b".repeat(64),
        rule_sha256: "c".repeat(64),
        split_windows_roots: Some(true),
    };
    let filename = format!("neighbor-reply-{question}.txt");
    for (mode, executable, restricted_cli) in [
        ("full-access", "domios", None),
        ("workspace-write", current, Some(policy)),
    ] {
        restored.settings.mode = Some(mode.into());
        restored.restricted_cli = restricted_cli;
        let direct = super::bound_turn_input(&mut restore(), &restored).unwrap();
        let frame = codeterm_wire::domios::parse_message(&direct.text).unwrap();
        assert_eq!(frame.body, "opaque body &lt;tip&gt;");
        assert!(direct.text.contains("CODETERM_PAD_DIR"));
        assert!(direct.text.contains(&format!(
            "{executable} account neighbors send '{friendship}' --kind answer --in-reply-to '{question}' --from-pad '{filename}'"
        )));
        assert!(!direct.text.contains(stale));
        assert!(!direct.text.contains(NEIGHBOR_REPLY_MCP_NAME));
        assert!(direct.attachments.is_empty());
    }
    restored.settings.mode = Some("read-only-with-writable-roots".into());
    restored.restricted_cli = None;
    for (mode, cli) in [
        ("read-only-with-writable-roots", current),
        ("workspace-write", "/private/tmp/current-server/codeterm"),
    ] {
        restored.settings.mode = Some(mode.into());
        restored.host_mcp = Some(RuntimeHostMcp {
            name: NEIGHBOR_REPLY_MCP_NAME.into(),
            command: cli.into(),
            args: vec![],
            env: std::collections::BTreeMap::from([(
                NEIGHBOR_REPLY_FRIENDSHIP_ENV.into(),
                friendship.clone(),
            )]),
            http: None,
        });
        let scoped = super::bound_turn_input(&mut restore(), &restored).unwrap();
        let frame = codeterm_wire::domios::parse_message(&scoped.text).unwrap();
        assert_eq!(frame.body, "opaque body &lt;tip&gt;");
        assert!(scoped.text.contains(&format!(
            "{NEIGHBOR_REPLY_MCP_TOOL} tool on {NEIGHBOR_REPLY_MCP_NAME}"
        )));
        assert!(scoped.text.contains(&format!("in_reply_to={question}")));
        assert!(!scoped.text.contains("account neighbors"));
        assert!(!scoped.text.contains("CODETERM_PAD_DIR"));
        assert!(!scoped.text.contains(stale));
        assert!(scoped.attachments.is_empty());
    }
}

#[test]
fn visible_preflight_reason_preserves_nonterminal_hold_classification() {
    let error = super::before_write(
        "own",
        super::RuntimeFailure::Refused("opaque reason".into()),
    );
    assert!(error
        .to_string()
        .contains("runtime_preflight_before_write: "));
    assert!(error.to_string().contains("opaque reason"));
    assert!(!super::rejected_before_write(&error));
}
#[test]
fn uncertain_handoff_cannot_replay_after_recovery_and_only_own_live_evidence_delivers() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab", "opaque payload", None).unwrap();
    let bound = SendLiveBinding {
        session_id: Some("thread".into()),
        binding_nonce: Some("nonce".into()),
    };
    assert!(ledger.bind_rpc_send(&pending.send_id, bound.clone()));
    ledger.mark_rpc_uncertain(&pending.send_id);
    assert!(!ledger.bind_rpc_send(&pending.send_id, bound));
    assert!(ledger
        .confirm_rpc_send(
            "other-tab",
            "thread",
            "nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab",
            "other-thread",
            "nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab",
            "thread",
            "old-nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            "tab",
            "thread",
            "nonce",
            &pending.send_id,
            Some("provider-item")
        )
        .is_some());
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Sent
    );
    ledger.finish(&pending.send_id, Err("late pipe failure".into()));
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Sent
    );
}
#[test]
fn acceptance_is_written_and_never_fabricates_delivery() {
    let ledger = ChatSendLedger::default();
    let pending = ledger.try_start("tab", "opaque payload", None).unwrap();
    assert!(ledger.bind_rpc_send(
        &pending.send_id,
        SendLiveBinding {
            session_id: Some("thread".into()),
            binding_nonce: Some("nonce".into())
        }
    ));
    ledger.mark_rpc_accepted(&pending.send_id);
    assert_eq!(
        ledger.status(&pending.send_id).unwrap().state,
        ChatSendState::Written
    );
}

#[test]
fn runtime_turn_input_carries_images_once_and_preserves_marker_text() {
    let text =
        "describe <domios type=\"attachment\" kind=\"image\" path=\"C:/images/a&amp;b.jpg\" />";
    let input = super::turn_input(&format!("{text}\n{text}"));
    assert_eq!(input.text, format!("{text}\n{text}"));
    assert_eq!(input.attachments.len(), 1);
    assert_eq!(
        input.attachments[0].path.as_deref(),
        Some("C:/images/a&b.jpg")
    );
    assert_eq!(input.attachments[0].mime_type, "image/jpeg");
    assert_eq!(input.attachments[0].id, "attachment:C:/images/a&b.jpg");
    for text in [
        "plain input",
        "<domios type=\"attachment\" kind=\"file\" path=\"/work/file.txt\" />",
        "`<domios type=\"attachment\" kind=\"image\" path=\"/example.png\" />`",
    ] {
        let input = super::turn_input(text);
        assert_eq!(input.text, text);
        assert!(input.attachments.is_empty());
    }
}

#[test]
fn runtime_image_bytes_are_complete_bounded_and_missing_files_refuse() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("image.png");
    let bytes = [137, 80, 78, 71, 0, 255];
    std::fs::write(&path, bytes).unwrap();
    let text = format!(
        "inspect <domios type=\"attachment\" kind=\"image\" path=\"{}\" />",
        path.to_string_lossy()
    );
    let loaded = super::prepare_image_input(super::turn_input(&text), false).unwrap();
    assert_eq!(loaded.text, text);
    assert_eq!(
        loaded.attachments[0].data.as_deref(),
        Some(base64::prelude::BASE64_STANDARD.encode(bytes).as_str())
    );
    std::fs::remove_file(&path).unwrap();
    for local_paths in [false, true] {
        let error = super::prepare_image_input(super::turn_input(&text), local_paths).unwrap_err();
        assert!(error
            .to_string()
            .contains("Image was not sent: Cannot open attachment"));
    }
    std::fs::write(&path, vec![1; super::MAX_RUNTIME_VALUE_BYTES]).unwrap();
    let error = super::prepare_image_input(super::turn_input(&text), false).unwrap_err();
    assert!(error.to_string().contains("Send fewer or smaller images"));
    std::fs::write(&path, []).unwrap();
    let error = super::prepare_image_input(super::turn_input(&text), true).unwrap_err();
    assert!(error.to_string().contains("Image was not sent: Empty file"));
}

#[test]
fn local_image_paths_keep_screenshot_bytes_out_of_claims_and_hook_arguments() {
    use codeterm_plugin::agent_runtime::{
        validate_runtime_size, ClaimedSend, RuntimeContext, RuntimeOperation,
    };
    let dir = tempfile::tempdir().unwrap();
    for count in [1, 2] {
        let bytes = if count == 1 { 3_500_000 } else { 1_600_000 };
        let mut text = String::from("inspect");
        for index in 0..count {
            let path = dir
                .path()
                .join(format!("Снимок экрана {index}\u{202f}PM.png"));
            std::fs::write(&path, vec![1; bytes]).unwrap();
            text.push_str(&format!(
                " <domios type=\"attachment\" kind=\"image\" path=\"{}\" />",
                path.display()
            ));
        }
        let input = super::prepare_image_input(super::turn_input(&text), true).unwrap();
        assert_eq!(input.attachments.len(), count);
        assert!(input
            .attachments
            .iter()
            .all(|image| image.data.is_none() && image.path.is_some()));
        for active_turn_id in [None, Some("active".to_string())] {
            let context = RuntimeContext {
                thread_id: Some("own".into()),
                active_turn_id: active_turn_id.clone(),
                claimed_sends: vec![ClaimedSend {
                    send_id: "send".into(),
                    thread_id: "own".into(),
                    input: input.clone(),
                }],
                ..Default::default()
            };
            let operation = RuntimeOperation::SubmitTurn {
                thread_id: "own".into(),
                active_turn_id,
                send_id: "send".into(),
                input: input.clone(),
                settings: Default::default(),
            };
            validate_runtime_size(
                &serde_json::json!([context, operation]),
                super::MAX_RUNTIME_VALUE_BYTES,
            )
            .unwrap();
        }
        assert!(super::prepare_image_input(super::turn_input(&text), false).is_err());
    }
}

#[test]
fn inline_images_can_fit_input_but_exceed_the_combined_hook_argument_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("image.png");
    std::fs::write(&path, vec![1; 1_600_000]).unwrap();
    let text = format!(
        "<domios type=\"attachment\" kind=\"image\" path=\"{}\" />",
        path.display()
    );
    let input = super::prepare_image_input(super::turn_input(&text), false).unwrap();
    super::validate_runtime_size(&input, super::MAX_RUNTIME_VALUE_BYTES).unwrap();
    let context = serde_json::json!({"claimedSends":[{"input":input}]});
    let operation = serde_json::json!({"kind":"submitTurn","input":input});
    assert!(super::validate_runtime_size(
        &serde_json::json!([context, operation]),
        super::MAX_RUNTIME_VALUE_BYTES
    )
    .is_err());
}

#[test]
fn remote_task_bound_tabs_disallow_silent_login_pending_and_detect_sign_in_errors() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    {
        let tab = mux.get_tab(&tab_id).unwrap();
        assert!(super::allows_login_pending(tab));
    }
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.report_target_mesh_peer = Some("origin-peer".into());
        assert!(super::allows_login_pending(tab));
        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some("plan-1".into()),
            task_id: Some("task-1".into()),
            attempt_id: Some("attempt-1".into()),
            spawn_handle: None,
            bound_at: 1,
        });
        assert!(!super::allows_login_pending(tab));
    }
    assert!(super::is_sign_in_required_launch_error(
        "Runtime tab 961f8af1 degraded: Sign-in required"
    ));
    assert!(!super::is_sign_in_required_launch_error(
        "Runtime tab 961f8af1 degraded: Pipe closed"
    ));
}
