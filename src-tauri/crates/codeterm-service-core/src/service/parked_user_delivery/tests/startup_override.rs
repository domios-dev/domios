use super::*;

#[test]
fn binding_deadline_owner_send_writes_once_and_own_append_retires_launch() {
    let (fixture, tab) = queue_fixture(true, None);
    attach_fixture_pty(&fixture, &tab);
    append_turn(&fixture, "opaque-owner-turn");
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert!(fixture.service.note_user_launch_dispatched(&authority));
    let attempt = crate::service::launch_readiness::LaunchAttempt {
        tab_id: tab.clone(),
        pty_id: authority.pty_id.clone().unwrap(),
        binding_nonce: authority.binding_nonce.clone(),
    };
    fixture.service.record_provisional_launch(
        &attempt,
        codeterm_types::pty::BootStage::LaunchBindingUnconfirmed,
        "launch binding deadline elapsed",
    );
    fixture
        .service
        .override_user_startup(&tab, &authority.binding_nonce)
        .unwrap();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "own-session evidence pending".into(),
            })
        }),
    );
    let sent = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("opaque-owner-turn"),
            None,
            Some("owner-once".into()),
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    fixture.service.drain_user_owner(&tab).unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert_eq!(
        fixture.service.mux.read().get_tab(&tab).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::LaunchBindingUnconfirmed)
    );
    let foreign_path = fixture
        .temp
        .path()
        .join("transcripts/foreign-session.jsonl");
    std::fs::write(foreign_path, "{\"content\":\"opaque-owner-turn\"}\n").unwrap();
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert!(!fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .launch_session_authenticated());
    append_turn(&fixture, "opaque-owner-turn");
    let turn = observed_turn(&fixture, &tab, "own-row", "opaque-owner-turn");
    assert!(deliver_turn(
        &fixture,
        &tab,
        "foreign-session",
        &authority.binding_nonce,
        &turn
    )
    .is_empty());
    assert!(deliver_turn(&fixture, &tab, "opaque-session", "foreign-nonce", &turn).is_empty());
    assert!(!fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .launch_session_authenticated());
    let original = fixture.service.mux.read().get_tab(&tab).unwrap().clone();
    for changed in ["pty", "nonce", "provider", "cwd", "session"] {
        {
            let mut mux = fixture.service.mux.write();
            let current = mux.get_tab_mut(&tab).unwrap();
            match changed {
                "pty" => current.pty_id = Some("foreign-pty".into()),
                "nonce" => current.session_binding_nonce = Some("foreign-nonce".into()),
                "provider" => current.agent_type = Some("foreign-provider".into()),
                "cwd" => current.cwd = Some("foreign-cwd".into()),
                "session" => current.agent_session_id = Some("foreign-session".into()),
                _ => unreachable!(),
            }
        }
        fixture
            .service
            .chat_send_ledger
            .reconcile_provisional_launch(&tab, None);
        assert!(!fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .launch_session_authenticated());
        *fixture.service.mux.write().get_tab_mut(&tab).unwrap() = original.clone();
    }
    fixture.service.reconcile_chat_send_evidence(&tab);
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab).unwrap();
        assert!(tab.launch_session_authenticated());
        assert_eq!(
            tab.boot_stage,
            Some(codeterm_types::pty::BootStage::AgentReady)
        );
        assert!(tab.provisional_launch.is_none());
    }
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&sent.send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
    fixture.service.drain_user_owner(&tab).unwrap();
    fixture.service.reconcile_chat_send_evidence(&tab);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

#[test]
fn explicit_override_flushes_held_sends_once_in_order_and_binding_retires_startup() {
    let (fixture, tab) = queue_fixture(true, None);
    let first = admit_current_ledger(&fixture.service, &tab, FIRST_PAYLOAD, "first-held");
    refuse_and_park_current_send(&fixture.service, first.clone(), FIRST_PAYLOAD);
    let second = admit_current_ledger(&fixture.service, &tab, SECOND_PAYLOAD, "second-held");
    refuse_and_park_current_send(&fixture.service, second.clone(), SECOND_PAYLOAD);
    attach_fixture_pty(&fixture, &tab);
    let (sent, received) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            sent.send((record.send_id.clone(), text.to_string()))
                .unwrap();
            Err(DeliveryError::SubmitUnconfirmed {
                tab: record.authority.tab_id.clone(),
                detail: "own-session evidence pending".into(),
            })
        }),
    );
    fixture.service.drain_user_owner(&tab).unwrap();
    await_owner_idle(&fixture, &tab);
    assert!(received.try_recv().is_err());
    let authority = fixture.service.user_send_authority(&tab).unwrap();
    assert!(fixture
        .service
        .override_user_startup(&tab, "retired-launch")
        .is_err());
    assert!(!fixture.service.user_authority_is_ready(&authority));
    fixture
        .service
        .pty
        .lock()
        .guarded_input_gate(authority.pty_id.as_deref().unwrap())
        .unwrap()
        .invalidate();
    let mut stale = authority.clone();
    stale.pty_session_generation = authority
        .pty_session_generation
        .map(|generation| generation + 1);
    assert!(!fixture.service.note_user_launch_dispatched(&stale));
    stale = authority.clone();
    stale.pty_id = Some("other-pty".into());
    assert!(!fixture.service.note_user_launch_dispatched(&stale));
    stale = authority.clone();
    stale.binding_nonce = "other-launch".into();
    assert!(!fixture.service.note_user_launch_dispatched(&stale));
    for stage in [
        codeterm_types::pty::BootStage::PtyStarting,
        codeterm_types::pty::BootStage::PtySpawned,
        codeterm_types::pty::BootStage::ReaderReady,
        codeterm_types::pty::BootStage::ShellReady,
        codeterm_types::pty::BootStage::AgentLaunching,
    ] {
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab)
            .unwrap()
            .boot_stage = Some(stage);
        assert!(fixture
            .service
            .override_user_startup(&tab, &authority.binding_nonce)
            .is_err());
        assert!(!fixture.service.user_authority_is_ready(&authority));
        assert!(fixture
            .service
            .pty
            .lock()
            .issue_guarded_input_permit(authority.pty_id.as_deref().unwrap())
            .is_err());
        assert!(received.try_recv().is_err());
    }
    assert!(fixture.service.note_user_launch_dispatched(&authority));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
    assert!(fixture
        .service
        .override_user_startup(&tab, &authority.binding_nonce)
        .is_err());
    assert!(received.try_recv().is_err());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .boot_stage = Some(codeterm_types::pty::BootStage::LaunchWritten);
    fixture
        .service
        .override_user_startup(&tab, &authority.binding_nonce)
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        received.recv_timeout(TEST_DEADLINE).unwrap(),
        (first.send_id, FIRST_PAYLOAD.replace("\r\n", "\n"))
    );
    assert_eq!(
        received.recv_timeout(TEST_DEADLINE).unwrap(),
        (second.send_id, format!("\n\n{SECOND_PAYLOAD}"))
    );
    fixture
        .service
        .override_user_startup(&tab, &authority.binding_nonce)
        .unwrap();
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(received.try_recv().is_err());
    assert_eq!(
        fixture.service.mux.read().get_tab(&tab).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::LaunchBindingUnconfirmed)
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .reported_session_binding_nonce = Some(authority.binding_nonce);
    fixture
        .service
        .reconcile_boot_stage_with_session_binding(&tab);
    assert_eq!(
        fixture.service.mux.read().get_tab(&tab).unwrap().boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    fixture.service.drain_user_owner(&tab).unwrap();
    assert!(received.try_recv().is_err());
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
}
