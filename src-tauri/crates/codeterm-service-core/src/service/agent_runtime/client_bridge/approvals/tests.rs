use super::super::tests as fixtures;
use super::*;
use tokio::io::{AsyncBufReadExt, BufReader};

fn permission(
    context: &RuntimeContext,
    path: &std::path::Path,
) -> (RuntimeContext, RuntimeOperation, Value) {
    let mut context = context.clone();
    context.pending_request = Some(PendingRuntimeRequest {
        request_id: RpcId::Number(90),
        method: "session/request_permission".into(),
        params: json!({
            "sessionId":context.thread_id,
            "toolCall":{"toolCallId":"opaque-tool","kind":"edit",
                "locations":[{"path":path}],
                "content":[{"type":"diff","path":path,"oldText":"","newText":"approved bytes"}]},
            "options":[{"optionId":"opaque-allow","name":"Allow","kind":"allow_once"},
                {"optionId":"opaque-reject","name":"Reject","kind":"reject_once"}]
        }),
    });
    let operation = RuntimeOperation::Answer {
        request_id: RpcId::Number(90),
        answer: PromptAnswer::Choice {
            values: vec!["opaque-allow".into()],
        },
    };
    (
        context,
        operation,
        json!({"outcome":{"outcome":"selected","optionId":"opaque-allow"}}),
    )
}

#[test]
fn carry_over_requires_an_approved_single_full_file_diff() {
    let context = fixtures::context(std::path::Path::new("/work"));
    let (context, operation, reply) = permission(&context, std::path::Path::new("/work/file"));
    assert_eq!(
        approved_write(&context, &operation, &reply),
        Some(("/work/file", "approved bytes"))
    );
    for (pointer, value) in [
        ("/method", json!("other/permission")),
        ("/params/sessionId", json!("foreign")),
        ("/params/toolCall/kind", json!("execute")),
        ("/params/toolCall/content/0/newText", Value::Null),
        ("/params/toolCall/locations/0/path", json!("/work/other")),
        ("/params/options/0/kind", json!("reject_once")),
    ] {
        let mut changed = context.clone();
        let mut pending = serde_json::to_value(changed.pending_request.as_ref().unwrap()).unwrap();
        *pending.pointer_mut(pointer).unwrap() = value;
        changed.pending_request = Some(serde_json::from_value(pending).unwrap());
        assert!(
            approved_write(&changed, &operation, &reply).is_none(),
            "{pointer}"
        );
    }
    let mut hidden = context.clone();
    hidden.pending_request.as_mut().unwrap().params["toolCall"]["rawInput"] =
        json!({"path":"/work/benign"});
    hidden.pending_request.as_mut().unwrap().params["toolCall"]["content"] = json!([
        {"type":"content","content":{"type":"text","text":"prose".repeat(3000)}},
        {"type":"content","content":{"type":"text","text":"more prose"}},
        {"type":"content","content":{"type":"text","text":"extra prose"}},
        {"type":"diff","path":"/work/file","oldText":"","newText":"approved bytes"}
    ]);
    assert!(approved_write(&hidden, &operation, &reply).is_none());
    let mut changed = context.clone();
    let request = changed.pending_request.as_mut().unwrap();
    let content = request.params["toolCall"]["content"]
        .as_array_mut()
        .unwrap();
    content.push(content[0].clone());
    assert!(approved_write(&changed, &operation, &reply).is_none());
    assert!(approved_write(
        &context,
        &RuntimeOperation::Answer {
            request_id: RpcId::Number(90),
            answer: PromptAnswer::Cancel,
        },
        &reply
    )
    .is_none());
    assert!(approved_write(
        &context,
        &operation,
        &json!({"outcome":{"outcome":"cancelled"}})
    )
    .is_none());
    let mut changed = context.clone();
    changed.pending_request.as_mut().unwrap().params["toolCall"]["content"][0]["newText"] =
        json!("x".repeat(fs::MAX_BYTES + 1));
    assert!(approved_write(&changed, &operation, &reply).is_none());
}

#[cfg(any(unix, windows))]
#[test]
fn exact_approved_write_is_consumed_once_and_persisted_without_another_prompt() {
    crate::async_runtime::block_on(async {
        const SOURCE: &str = r#"module.exports = {
            runtimePlan(ctx, op) {
                return {action:{kind:'reply',requestId:op.requestId,result:{kind:'success',
                    value:{outcome:{outcome:'selected',optionId:op.answer.values[0]}}}},state:null};
            },
            runtimeResponse() { return {kind:'complete',result:{kind:'done'}}; },
            runtimeReduce() { return {state:null,events:[]}; }
        };"#;
        let (_temp, root) = fixtures::root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture_with(SOURCE);
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut context = session.context.lock();
            context.cwd = root.to_string_lossy().into_owned();
            context.active_turn_id = Some("own-turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(fixtures::capabilities())).unwrap();
        let bridge = bridge(&context).unwrap();
        let path = root.join("created.txt");
        let (permission_context, operation, _) = permission(&context, &path);
        let pending = permission_context.pending_request.clone().unwrap();
        let prompt: PendingPrompt = serde_json::from_value(json!({
            "question":"opaque permission","options":[{"label":"Allow","value":"opaque-allow"}]
        }))
        .unwrap();
        session.prompts.lock().insert(
            pending.request_id.clone(),
            engine::OutstandingRequest {
                request: pending.clone(),
                prompt: Some(prompt.clone()),
                permission: None,
            },
        );
        super::super::super::prompt_answers::publish(
            &fixture.service,
            &session,
            pending.request_id.clone(),
            prompt,
        );
        let process = session.process.lock().clone().unwrap();
        engine::run(
            &fixture.service,
            &session.provider,
            &session,
            &process,
            operation,
        )
        .await
        .unwrap();
        super::super::super::prompt_answers::resolve(
            &fixture.service,
            &session,
            &pending.request_id,
        );
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["result"]["outcome"]["optionId"],
            "opaque-allow"
        );
        for (candidate, content) in [
            (path.clone(), "changed bytes"),
            (root.join("other.txt"), "approved bytes"),
        ] {
            assert!(prepare(
                &fixture.service,
                &session,
                &context,
                &bridge,
                &Request::Write {
                    session_id: "own-thread".into(),
                    path: candidate.to_string_lossy().into_owned(),
                    content: content.into(),
                }
            )
            .unwrap()
            .0
            .is_some());
        }
        handle(
            &fixture.service,
            &session,
            &process,
            &RpcId::Number(91),
            "fs/write_text_file",
            &json!({"sessionId":"own-thread","path":path,"content":"approved bytes"}),
        )
        .await
        .unwrap();
        assert!(bridge.state.lock().pending.is_empty());
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"id":91,"result":{}})
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "approved bytes");
        assert!(prepare(
            &fixture.service,
            &session,
            &context,
            &bridge,
            &Request::Write {
                session_id: "own-thread".into(),
                path: path.to_string_lossy().into_owned(),
                content: "approved bytes".into(),
            }
        )
        .unwrap()
        .0
        .is_some());
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}

#[cfg(any(unix, windows))]
#[test]
fn carry_over_rejects_stale_scope_and_is_revoked_on_new_turn() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = fixtures::root();
        let context = fixtures::context(&root);
        install(&context, Some(fixtures::capabilities())).unwrap();
        let bridge = bridge(&context).unwrap();
        let path = root.join("file");
        let (permission, operation, reply) = permission(&context, &path);
        let request = &permission.pending_request.as_ref().unwrap().request_id;
        let mint = || async {
            remember(&context, request);
            approved_reply(&permission, &operation, Some(&reply))
                .await
                .unwrap()
        };
        let id = mint().await;
        for changed in [
            RuntimeContext {
                epoch: context.epoch + 1,
                ..context.clone()
            },
            RuntimeContext {
                binding_nonce: "foreign".into(),
                ..context.clone()
            },
            RuntimeContext {
                thread_id: Some("foreign".into()),
                ..context.clone()
            },
            RuntimeContext {
                active_turn_id: Some("next-turn".into()),
                ..context.clone()
            },
        ] {
            assert!(!consume(
                &bridge,
                &changed,
                path.to_str().unwrap(),
                "approved bytes"
            ));
        }
        revoke(&context, Some(id));
        assert!(!consume(
            &bridge,
            &context,
            path.to_str().unwrap(),
            "approved bytes"
        ));
        mint().await;
        let reject = RuntimeOperation::Answer {
            request_id: RpcId::Number(90),
            answer: PromptAnswer::Choice {
                values: vec!["opaque-reject".into()],
            },
        };
        assert!(approved_reply(
            &permission,
            &reject,
            Some(&json!({"outcome":{"outcome":"selected","optionId":"opaque-reject"}}))
        )
        .await
        .is_none());
        assert!(!consume(
            &bridge,
            &context,
            path.to_str().unwrap(),
            "approved bytes"
        ));
        mint().await;
        forget(&context, None);
        assert!(!consume(
            &bridge,
            &context,
            path.to_str().unwrap(),
            "approved bytes"
        ));
        mint().await;
        begin_turn(&context);
        assert!(!consume(
            &bridge,
            &context,
            path.to_str().unwrap(),
            "approved bytes"
        ));
        remember(&context, request);
        let mut next = permission.clone();
        next.active_turn_id = Some("next-turn".into());
        assert!(approved_reply(&next, &operation, Some(&reply))
            .await
            .is_none());
        BRIDGES.lock().remove(&context.tab_id);
    });
}
