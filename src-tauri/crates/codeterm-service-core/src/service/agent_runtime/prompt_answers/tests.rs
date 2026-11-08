use super::*;

fn prompt() -> PendingPrompt {
    serde_json::from_value(serde_json::json!({"question":"opaque", "options":[{"label":"Allow", "value":"native-yes"},{"label":"Reject", "value":"native-no"}]})).unwrap()
}

#[test]
fn choices_send_native_values_and_reject_unknown_options() {
    assert_eq!(
        answer(&prompt(), "1").unwrap(),
        PromptAnswer::Choice {
            values: vec!["native-yes".into()]
        }
    );
    assert_eq!(
        answer(&prompt(), "native-no").unwrap(),
        PromptAnswer::Choice {
            values: vec!["native-no".into()]
        }
    );
    assert!(answer(&prompt(), "foreign").is_err());
}

#[test]
fn multiselect_keeps_opaque_values_and_refuses_foreign_values() {
    let mut prompt = prompt();
    prompt.multi_select = Some(true);
    assert_eq!(
        answer(&prompt, r#"["native-yes","native-no"]"#).unwrap(),
        PromptAnswer::Choice {
            values: vec!["native-yes".into(), "native-no".into()]
        }
    );
    assert!(answer(&prompt, r#"["other"]"#).is_err());
    assert!(answer(&prompt, "10001").is_err());
}
#[test]
fn exact_request_identity_rejects_epoch_binding_and_typed_id_changes() {
    let context = RuntimeContext {
        restricted_cli: None,
        epoch: 2,
        binding_nonce: "own".into(),
        ..Default::default()
    };
    let id = RpcId::Number(7);
    let mut prompt = prompt();
    prompt.request_id = Some(request_token(&context, &id));
    let identity = PromptIdentity::new(PromptKind::Structured, &prompt);
    let mut entry = Entry {
        request: id,
        epoch: 2,
        binding: "own".into(),
        thread: context.thread_id.clone(),
        prompt,
        claimed: false,
    };
    assert!(matching_entry(&entry, &context, &identity));
    assert!(!matching_entry(
        &entry,
        &RuntimeContext {
            restricted_cli: None,
            epoch: 3,
            ..context.clone()
        },
        &identity
    ));
    assert!(!matching_entry(
        &entry,
        &RuntimeContext {
            restricted_cli: None,
            binding_nonce: "foreign".into(),
            ..context.clone()
        },
        &identity
    ));
    entry.thread = Some("foreign-session".into());
    assert!(!matching_entry(&entry, &context, &identity));
    entry.thread = context.thread_id.clone();
    entry.request = RpcId::String("7".into());
    assert!(!matching_entry(&entry, &context, &identity));
    entry.request = RpcId::Number(8);
    assert!(!matching_entry(&entry, &context, &identity));
    entry.request = RpcId::Number(7);
    entry.claimed = true;
    assert!(!matching_entry(&entry, &context, &identity));
}

#[test]
fn numeric_multiselect_rejects_unknown_bits_and_preserves_native_values() {
    let mut prompt = prompt();
    prompt.multi_select = Some(true);
    prompt.options[0].value = "1".into();
    prompt.options[1].value = "3".into();
    assert_eq!(
        answer(&prompt, "10005").unwrap(),
        PromptAnswer::Choice {
            values: vec!["1".into(), "3".into()]
        }
    );
    assert!(answer(&prompt, "10002").is_err());
}

#[test]
fn a_waiting_permission_request_follows_the_switch_to_allow_all_and_ask_leaves_it_waiting() {
    use codeterm_plugin::agent_runtime::{
        PendingRuntimeRequest, RuntimePermission, RuntimePermissionAction,
        RuntimePermissionDecision, RuntimePermissionOption,
    };
    use tokio::io::{AsyncBufReadExt, BufReader};
    crate::async_runtime::block_on(async {
        let source = r#"module.exports = {
            runtimePlan(ctx, op) {
                if (op.kind !== 'answer' || op.answer.kind !== 'choice') throw new Error('Expected a chosen answer');
                return {action:{kind:'reply',requestId:op.requestId,result:{kind:'success',value:{selected:op.answer.values[0]}}},state:null};
            },
            runtimeResponse() { return {kind:'complete',result:{kind:'done'}}; },
            runtimeReduce(ctx,state,frames) { return {state:null,events:[]}; }
        };"#;
        let (fixture, tab, peer, _cleanup) =
            engine::test_support::runtime_fixture_host_approvals_with(source);
        let _storage = fixture.install_seeded_storage_desk();
        let svc = &fixture.service;
        svc.set_tab_host_approvals(&tab, false).unwrap();
        let session = engine::for_tab(&tab).unwrap();
        let request = RpcId::Number(21);
        let shown = prompt();
        session.prompts.lock().insert(
            request.clone(),
            engine::OutstandingRequest {
                request: PendingRuntimeRequest {
                    request_id: request.clone(),
                    method: "session/request_permission".into(),
                    params: serde_json::json!({"sessionId":"own-thread"}),
                },
                prompt: Some(shown.clone()),
                permission: Some(RuntimePermission {
                    action: RuntimePermissionAction::Command,
                    options: vec![
                        RuntimePermissionOption {
                            value: "native-yes".into(),
                            decision: RuntimePermissionDecision::AllowOnce,
                        },
                        RuntimePermissionOption {
                            value: "native-no".into(),
                            decision: RuntimePermissionDecision::RejectOnce,
                        },
                    ],
                }),
            },
        );
        publish(svc, &session, request.clone(), shown);

        settle_after_policy_change(svc, &tab);
        assert!(
            session.prompts.lock().contains_key(&request),
            "the Ask choice leaves the request with the owner"
        );
        assert!(svc
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .pending_prompt
            .is_some());

        svc.set_tab_host_approvals(&tab, true).unwrap();
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let frame: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(frame["id"], serde_json::json!(21));
        assert_eq!(
            frame["result"],
            serde_json::json!({"selected":"native-yes"})
        );
    });
}

#[test]
fn pending_requests_cancelled_before_interrupt_and_prompts_resolved() {
    use codeterm_plugin::agent_runtime::{PendingRuntimeRequest, RuntimeClientBridges};
    use tokio::io::{AsyncBufReadExt, BufReader};
    crate::async_runtime::block_on(async {
        let source = r#"module.exports = {
            runtimePlan(ctx, op) {
                if (op.kind !== 'answer' || op.answer.kind !== 'cancel' || ctx.pendingRequest.requestId !== op.requestId) throw new Error('Expected exact pending cancellation');
                return {action:{kind:'reply',requestId:op.requestId,result:{kind:'success',value:{outcome:{outcome:'cancelled'}}}},state:null};
            },
            runtimeResponse() { return {kind:'complete',result:{kind:'done'}}; },
            runtimeReduce(ctx,state,frames) { return {state:null,events:[]}; }
        };"#;
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture_with(source);
        let session = engine::for_tab(&tab).unwrap();
        session.context.lock().client_bridges = Some(RuntimeClientBridges {
            fs_read: true,
            fs_write: true,
            terminal: true,
        });
        let process = session.process.lock().clone().unwrap();
        for request in [RpcId::Number(11), RpcId::String("opaque-id".into())] {
            let shown = prompt();
            session.prompts.lock().insert(
                request.clone(),
                engine::OutstandingRequest {
                    request: PendingRuntimeRequest {
                        request_id: request.clone(),
                        method: "session/request_permission".into(),
                        params: serde_json::json!({"sessionId":"own-thread"}),
                    },
                    prompt: Some(shown.clone()),
                    permission: None,
                },
            );
            publish(&fixture.service, &session, request, shown);
        }
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .pending_prompt
            .is_some());
        cancel_pending(&fixture.service, &session, &process)
            .await
            .unwrap();
        assert!(session.prompts.lock().is_empty());
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .pending_prompt
            .is_none());
        let mut reader = BufReader::new(peer);
        let mut ids = Vec::new();
        for _ in 0..2 {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let frame: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                frame["result"],
                serde_json::json!({"outcome":{"outcome":"cancelled"}})
            );
            ids.push(frame["id"].clone());
        }
        assert!(ids.contains(&serde_json::json!(11)));
        assert!(ids.contains(&serde_json::json!("opaque-id")));
    });
}
