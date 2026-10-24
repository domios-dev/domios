use super::super::tests::sync_send::runtime_fixture_with;
use super::*;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROVIDER: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind === 'resumeThread' ? 'fixture/resume' : 'fixture/open',params:{}}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'resumeThread') return {kind:'refused',reason:{code:'thread_not_found',message:'opaque-vendor-diagnostic'},beforeAcceptance:true};
    return {kind:'complete',result:{kind:'threadLoaded',threadId:'replacement-thread',settings:op.settings}};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_rollout_replaces_only_a_proven_unused_thread_without_sending_input() {
    for evidence in ["unused", "used", "unknown", "foreign"] {
        let never_used = evidence == "unused";
        let (fixture, tab, peer, _cleanup) = runtime_fixture_with(PROVIDER);
        let session = for_tab(&tab).unwrap();
        if never_used {
            mark_unused(&fixture.service, &session, "own-thread").unwrap();
        } else if evidence == "used" {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab)
                .unwrap()
                .agent_runtime_config = Some(json!({"usedThreadId":"own-thread"}));
        } else if evidence == "foreign" {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab)
                .unwrap()
                .agent_runtime_config = Some(json!({"unusedThreadId":"foreign-thread"}));
        }
        let server = tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(peer);
            let mut lines = BufReader::new(reader).lines();
            for expected in if never_used {
                vec!["fixture/resume", "fixture/open"]
            } else {
                vec!["fixture/resume"]
            } {
                let request: Value =
                    serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
                assert_eq!(request["method"], expected);
                let reply = json!({"jsonrpc":"2.0","id":request["id"],"result":{}});
                writer
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let process = session.process.lock().clone().unwrap();
        let result = load(
            &fixture.service,
            &session.provider,
            &session,
            &process,
            RuntimeOperation::ResumeThread {
                thread_id: "own-thread".into(),
                instructions: "opaque-instructions".into(),
                settings: RuntimeSettings::default(),
            },
        )
        .await;
        server.await.unwrap();
        if never_used {
            assert!(
                matches!(result, Ok(RuntimeResult::ThreadLoaded { thread_id, .. }) if thread_id == "replacement-thread")
            );
            assert_eq!(session.context.lock().thread_id, None);
            assert_eq!(
                fixture
                    .service
                    .mux
                    .read()
                    .get_tab(&tab)
                    .unwrap()
                    .agent_session_id,
                None
            );
            session.context.lock().thread_id = Some("replacement-thread".into());
            bind(&fixture.service, &session, "replacement-thread").unwrap();
            let epoch = session.context.lock().epoch;
            assert!(session.state.lock().load(epoch, "replacement-thread"));
            validate_binding(&fixture.service, &session).unwrap();
            assert_eq!(
                fixture
                    .service
                    .mux
                    .read()
                    .get_tab(&tab)
                    .unwrap()
                    .agent_session_id
                    .as_deref(),
                Some("replacement-thread")
            );
        } else {
            assert_eq!(result, Err(RuntimeFailure::ConversationMissing));
            assert_eq!(
                session.context.lock().thread_id.as_deref(),
                Some("own-thread")
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_turn_durably_removes_unused_evidence_before_any_provider_write() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_with(PROVIDER);
    let session = for_tab(&tab).unwrap();
    mark_unused(&fixture.service, &session, "own-thread").unwrap();
    let storage = fixture.install_seeded_storage_desk();
    let adapter = crate::session::store::StorageDeskAdapter::from_storage(storage);
    before_turn(&fixture.service, &session).await.unwrap();
    let persisted = crate::multiplexer::state::MultiplexerState::from_session_snapshot(
        &adapter.snapshot().unwrap(),
    );
    assert!(!unused(
        persisted
            .get_tab(&tab)
            .unwrap()
            .agent_runtime_config
            .as_ref()
            .unwrap(),
        "own-thread"
    ));
    let mux = fixture.service.mux.read();
    let config = mux
        .get_tab(&tab)
        .unwrap()
        .agent_runtime_config
        .as_ref()
        .unwrap();
    assert!(!unused(config, "own-thread"));
    assert!(config.get("usedThreadId").is_none());
}
