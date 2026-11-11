use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};

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
