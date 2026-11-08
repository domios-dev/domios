use super::*;

#[test]
fn launch_policy_allows_workspace_operations_and_read_only_refuses_even_with_grants() {
    let mut grants = SessionGrants::default();
    assert_eq!(
        grants.decide(Access::WorkspaceWrite, Kind::Write),
        Decision::Allow
    );
    for kind in [Kind::Write, Kind::Command] {
        assert_eq!(grants.decide(Access::Full, kind), Decision::Allow);
        assert_eq!(grants.decide(Access::Interactive, kind), Decision::Prompt);
        grants.grant(kind);
        assert_eq!(grants.decide(Access::ReadOnly, kind), Decision::Refuse);
    }
}

#[test]
fn workspace_write_refuses_unconfined_commands_even_with_a_session_grant() {
    let mut grants = SessionGrants::default();
    assert_eq!(
        grants.decide(Access::WorkspaceWrite, Kind::Command),
        Decision::Refuse
    );
    grants.grant(Kind::Command);
    assert_eq!(
        grants.decide(Access::WorkspaceWrite, Kind::Command),
        Decision::Refuse
    );
    assert_eq!(grants.decide(Access::Full, Kind::Command), Decision::Allow);
    assert_eq!(
        grants.decide(Access::WorkspaceWrite, Kind::Write),
        Decision::Allow
    );
}

#[test]
fn session_grants_are_separate_for_writes_commands_and_sensitive_reads() {
    for kind in [Kind::Write, Kind::Command, Kind::SensitiveRead] {
        let mut grants = SessionGrants::default();
        grants.grant(kind);
        for candidate in [Kind::Write, Kind::Command, Kind::SensitiveRead] {
            assert_eq!(
                grants.decide(Access::Interactive, candidate),
                if candidate == kind {
                    Decision::Allow
                } else {
                    Decision::Prompt
                }
            );
        }
    }
}

#[test]
fn new_tab_epoch_binding_or_thread_clears_grants_but_turn_change_preserves_them() {
    let context = RuntimeContext {
        tab_id: "tab".into(),
        epoch: 3,
        binding_nonce: "binding".into(),
        thread_id: Some("thread".into()),
        active_turn_id: Some("turn".into()),
        ..Default::default()
    };
    for changed in [
        RuntimeContext {
            tab_id: "other".into(),
            ..context.clone()
        },
        RuntimeContext {
            epoch: 4,
            ..context.clone()
        },
        RuntimeContext {
            binding_nonce: "new".into(),
            ..context.clone()
        },
        RuntimeContext {
            thread_id: Some("new".into()),
            ..context.clone()
        },
    ] {
        let mut grants = SessionGrants::default();
        grants.bind(&context);
        grants.grant(Kind::Write);
        grants.bind(&RuntimeContext {
            active_turn_id: Some("next".into()),
            ..context.clone()
        });
        assert_eq!(
            grants.decide(Access::Interactive, Kind::Write),
            Decision::Allow
        );
        grants.bind(&changed);
        assert_eq!(
            grants.decide(Access::Interactive, Kind::Write),
            Decision::Prompt
        );
        grants.bind(&context);
        assert_eq!(
            grants.decide(Access::Interactive, Kind::Write),
            Decision::Prompt
        );
    }
}

#[test]
fn prompt_answers_accept_only_single_exact_allow_choices() {
    let prompt = prompt("opaque".into()).unwrap();
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        ["allow_once", "allow_session", "reject_once"]
    );
    for (values, allowed, session) in [
        (vec!["allow_once"], true, false),
        (vec!["allow_session"], true, true),
        (vec!["reject_once"], false, false),
        (vec!["allow_once", "allow_session"], false, false),
        (vec!["allow_always"], false, false),
        (vec![], false, false),
    ] {
        let answer = PromptAnswer::Choice {
            values: values.into_iter().map(String::from).collect(),
        };
        assert_eq!(allowed_answer(&answer), allowed);
        assert_eq!(session_answer(&answer), session);
    }
    assert!(!allowed_answer(&PromptAnswer::Cancel));
}

#[cfg(any(unix, windows))]
#[test]
fn permitting_launch_skips_write_prompt_but_preserves_hard_refusals() {
    let (_temp, root) = super::super::tests::root();
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    session.context.lock().cwd = root.to_string_lossy().into_owned();
    let context = session.context.lock().clone();
    install(&context, Some(super::super::tests::capabilities())).unwrap();
    let bridge = bridge(&context).unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restorable_launch
        .permission_bypass_requested = true;
    let write = Request::Write {
        session_id: context.thread_id.clone().unwrap(),
        path: root.join("new.txt").to_string_lossy().into_owned(),
        content: "opaque".into(),
    };
    assert!(
        prepare(&fixture.service, &session, &context, &bridge, &write)
            .unwrap()
            .0
            .is_none()
    );
    bridge.state.lock().session_grants.grant(Kind::Write);
    bridge.state.lock().session_grants.grant(Kind::Command);
    for request in [
        Request::Write { session_id: context.thread_id.clone().unwrap(), path: root.join("new.txt").to_string_lossy().into_owned(), content: "x".repeat(fs::MAX_BYTES + 1) },
        Request::Write { session_id: context.thread_id.clone().unwrap(), path: root.join("../escape").to_string_lossy().into_owned(), content: "opaque".into() },
        Request::Write { session_id: context.thread_id.clone().unwrap(), path: root.join(".domios/credentials.json").to_string_lossy().into_owned(), content: "opaque".into() },
        serde_json::from_value(json!({"method":"terminal/create","params":{"sessionId":context.thread_id,"command":"bad\ncommand"}})).unwrap(),
        serde_json::from_value(json!({"method":"terminal/create","params":{"sessionId":context.thread_id,"command":"tool","args":vec!["arg";129]}})).unwrap(),
        serde_json::from_value(json!({"method":"terminal/create","params":{"sessionId":context.thread_id,"command":"tool","cwd":root.join("../escape")}})).unwrap(),
    ] {
        assert!(prepare(&fixture.service, &session, &context, &bridge, &request).is_err());
    }
    let policy = codeterm_types::plan::ExecutionSandboxPolicy {
        intent: ExecutionIntent::Mutating,
        cwd: context.cwd.clone(),
        launch_args: vec![],
        runtime_mode: context.settings.mode.clone(),
        writable_roots: vec![],
        environment: Default::default(),
    };
    let mut confined = context.clone();
    confined.settings.network_access = Some(false);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restorable_launch
        .execution_sandbox = Some(policy);
    assert!(
        prepare(&fixture.service, &session, &confined, &bridge, &write)
            .unwrap()
            .0
            .is_none()
    );
    let command = serde_json::from_value(json!({"method":"terminal/create","params":{
        "sessionId":confined.thread_id,"command":"tool","cwd":root
    }}))
    .unwrap();
    assert_eq!(
        prepare(&fixture.service, &session, &confined, &bridge, &command)
            .err()
            .unwrap()
            .to_string(),
        "Execution sandbox denies client operation"
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restorable_launch
        .execution_sandbox
        .as_mut()
        .unwrap()
        .intent = ExecutionIntent::ReadOnly;
    assert!(prepare(&fixture.service, &session, &confined, &bridge, &write).is_err());
    BRIDGES.lock().remove(&context.tab_id);
}

#[cfg(any(unix, windows))]
#[test]
fn switching_to_allow_all_answers_the_waiting_request_and_ask_prompts_again() {
    crate::async_runtime::block_on(async {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let (_temp, root) = super::super::tests::root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture_host_approvals();
        let _storage = fixture.install_seeded_storage_desk();
        let svc = &fixture.service;
        svc.set_tab_host_approvals(&tab, false).unwrap();
        assert!(!svc.effective_full_access(&tab));
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut context = session.context.lock();
            context.cwd = root.to_string_lossy().into_owned();
            context.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(super::super::tests::capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        handle(
            svc,
            &session,
            &process,
            &RpcId::Number(501),
            "fs/write_text_file",
            &json!({"sessionId":context.thread_id,"path":root.join("waiting.txt"),"content":"waiting"}),
        )
        .await
        .unwrap();
        assert!(!root.join("waiting.txt").exists());

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
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"id":501,"result":{}})
        );
        assert_eq!(
            std::fs::read_to_string(root.join("waiting.txt")).unwrap(),
            "waiting"
        );
        assert!(svc.effective_full_access(&tab));

        svc.set_tab_host_approvals(&tab, false).unwrap();
        let installed = bridge(&context).unwrap();
        let write = Request::Write {
            session_id: context.thread_id.clone().unwrap(),
            path: root.join("next.txt").to_string_lossy().into_owned(),
            content: "next".into(),
        };
        assert!(prepare(svc, &session, &context, &installed, &write)
            .unwrap()
            .0
            .is_some());
        cancel(svc, &session, &process, true).await.unwrap();
    });
}

#[cfg(any(unix, windows))]
#[test]
fn auto_confirm_pending_bridge_uses_allow_once_and_does_not_grant_session() {
    crate::async_runtime::block_on(async {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let (_temp, root) = super::super::tests::root();
        let (fixture, tab, peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut context = session.context.lock();
            context.cwd = root.to_string_lossy().into_owned();
            context.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(super::super::tests::capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        handle(
            &fixture.service,
            &session,
            &process,
            &RpcId::Number(401),
            "fs/write_text_file",
            &json!({"sessionId":context.thread_id,"path":root.join("first.txt"),"content":"first"}),
        )
        .await
        .unwrap();
        assert!(!root.join("first.txt").exists());
        fixture
            .service
            .set_permission_auto_confirm(&tab, true)
            .unwrap();
        let mut reader = BufReader::new(peer);
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"id":401,"result":{}})
        );
        assert_eq!(
            std::fs::read_to_string(root.join("first.txt")).unwrap(),
            "first"
        );
        handle(&fixture.service, &session, &process, &RpcId::Number(402), "fs/write_text_file",
            &json!({"sessionId":context.thread_id,"path":root.join("second.txt"),"content":"second"})).await.unwrap();
        line.clear();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"id":402,"result":{}})
        );
        assert_eq!(
            std::fs::read_to_string(root.join("second.txt")).unwrap(),
            "second"
        );
        fixture
            .service
            .set_permission_auto_confirm(&tab, false)
            .unwrap();
        let installed = bridge(&context).unwrap();
        let write = Request::Write {
            session_id: context.thread_id.clone().unwrap(),
            path: root.join("third.txt").to_string_lossy().into_owned(),
            content: "third".into(),
        };
        assert!(
            prepare(&fixture.service, &session, &context, &installed, &write)
                .unwrap()
                .0
                .is_some()
        );
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}

#[cfg(any(unix, windows))]
#[test]
fn session_answer_survives_turns_and_bridge_restart_requires_approval_again() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = super::super::tests::root();
        let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        {
            let mut context = session.context.lock();
            context.cwd = root.to_string_lossy().into_owned();
            context.active_turn_id = Some("turn".into());
        }
        let context = session.context.lock().clone();
        install(&context, Some(super::super::tests::capabilities())).unwrap();
        let process = session.process.lock().clone().unwrap();
        let id = RpcId::Number(403);
        handle(
            &fixture.service,
            &session,
            &process,
            &id,
            "fs/write_text_file",
            &json!({"sessionId":context.thread_id,"path":root.join("first.txt"),"content":"first"}),
        )
        .await
        .unwrap();
        answer(
            &fixture.service,
            &session,
            &process,
            &id,
            &PromptAnswer::Choice {
                values: vec!["allow_session".into()],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("first.txt")).unwrap(),
            "first"
        );
        let installed = bridge(&context).unwrap();
        let write = Request::Write {
            session_id: context.thread_id.clone().unwrap(),
            path: root.join("second.txt").to_string_lossy().into_owned(),
            content: "second".into(),
        };
        assert!(
            prepare(&fixture.service, &session, &context, &installed, &write)
                .unwrap()
                .0
                .is_none()
        );
        cancel(&fixture.service, &session, &process, false)
            .await
            .unwrap();
        begin_turn(&context);
        assert!(
            prepare(&fixture.service, &session, &context, &installed, &write)
                .unwrap()
                .0
                .is_none()
        );
        assert_eq!(
            installed
                .state
                .lock()
                .session_grants
                .decide(Access::Interactive, Kind::Command),
            Decision::Prompt
        );
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
        install(&context, Some(super::super::tests::capabilities())).unwrap();
        let restarted = bridge(&context).unwrap();
        assert!(
            prepare(&fixture.service, &session, &context, &restarted, &write)
                .unwrap()
                .0
                .is_some()
        );
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}
