use super::*;
use crate::agent::test_provider::MockProvider;
use crate::service::pending_delivery_payloads::user_sends::UserSendAuthority;
use crate::service::test_fixture::ServiceFixture;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(10);

fn fixture() -> (
    ServiceFixture,
    TabRestartSnapshot,
    ModelSwitchLaunch,
    UserSendAuthority,
) {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("lazy-race-fixture".into());
    tab.backing = crate::multiplexer::types::TabBacking::Pty;
    tab.pty_id = Some("old-pty".into());
    tab.session_binding_nonce = Some("old-nonce".into());
    tab.cwd = Some(std::env::temp_dir().display().to_string());
    tab.restorable_launch.model = Some("vendor/prior-model".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("lazy-race-fixture")
            .with_session_starts_on_first_user_turn()
            .with_model_switch(ModelSwitchMode::RestartResume),
    ));
    let attempt = LaunchAttempt::current(&fixture.service.mux, &id, "old-pty").unwrap();
    fixture
        .service
        .record_first_turn_input_ready(&attempt, "opaque composer evidence");
    let snap = fixture.service.snapshot_tab_restart_state(&id).unwrap();
    let launch =
        ModelSwitchLaunch::prepare(&fixture.service, &snap, "", "vendor/next-model").unwrap();
    let authority = fixture.service.user_send_authority(&id).unwrap();
    (fixture, snap, launch, authority)
}

#[test]
fn claimed_first_send_refuses_contending_switch_before_teardown_or_commit() {
    let (fixture, snap, launch, authority) = fixture();
    let writes = AtomicUsize::new(0);
    let teardowns = AtomicUsize::new(0);
    let commits = AtomicUsize::new(0);
    let (claimed_tx, claimed_rx) = mpsc::sync_channel(1);
    let (write_tx, write_rx) = mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let svc = fixture.service.clone();
        let writes = &writes;
        let sender = scope.spawn(move || {
            let claimed = svc.claim_lazy_first_turn_input(&authority, "human-send");
            claimed_tx.send(claimed).unwrap();
            write_rx.recv_timeout(DEADLINE).unwrap();
            if claimed {
                writes.fetch_add(1, Ordering::SeqCst);
            }
        });
        assert!(claimed_rx.recv_timeout(DEADLINE).unwrap());
        let result = launch.teardown_sessionless_switch(
            &fixture.service,
            &snap,
            || {
                teardowns.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || {
                commits.fetch_add(1, Ordering::SeqCst);
                launch.commit(&fixture.service, &snap.tab_id);
            },
        );
        assert!(matches!(
            result,
            Err(SessionlessTeardownError::Admission(_))
        ));
        write_tx.send(()).unwrap();
        sender.join().unwrap();
    });
    assert_eq!(teardowns.load(Ordering::SeqCst), 0);
    assert_eq!(commits.load(Ordering::SeqCst), 0);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&snap.tab_id)
        .unwrap()
        .clone();
    assert_eq!(tab.session_binding_nonce.as_deref(), Some("old-nonce"));
    assert_eq!(
        tab.restorable_launch.model.as_deref(),
        Some("vendor/prior-model")
    );
}

#[test]
fn reserved_switch_fences_old_first_send_before_teardown_and_new_input_uses_new_attempt() {
    let (fixture, snap, launch, authority) = fixture();
    let writes = AtomicUsize::new(0);
    let teardowns = AtomicUsize::new(0);
    let commits = AtomicUsize::new(0);
    let (reserved_tx, reserved_rx) = mpsc::sync_channel(1);
    let (claimed_tx, claimed_rx) = mpsc::sync_channel::<bool>(1);
    std::thread::scope(|scope| {
        let svc = &fixture.service;
        let launch = &launch;
        let snap = &snap;
        let teardowns = &teardowns;
        let commits = &commits;
        let switch = scope.spawn(move || {
            launch.teardown_sessionless_switch(
                svc,
                snap,
                || {
                    reserved_tx.send(()).unwrap();
                    assert!(!claimed_rx.recv_timeout(DEADLINE).unwrap());
                    teardowns.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                || {
                    commits.fetch_add(1, Ordering::SeqCst);
                    launch.commit(svc, &snap.tab_id);
                },
            )
        });
        reserved_rx.recv_timeout(DEADLINE).unwrap();
        let claimed = fixture
            .service
            .claim_lazy_first_turn_input(&authority, "old-human-send");
        if claimed {
            writes.fetch_add(1, Ordering::SeqCst);
        }
        claimed_tx.send(claimed).unwrap();
        switch.join().unwrap().unwrap();
    });
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(teardowns.load(Ordering::SeqCst), 1);
    assert_eq!(commits.load(Ordering::SeqCst), 1);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&snap.tab_id).unwrap();
        assert_ne!(tab.session_binding_nonce.as_deref(), Some("old-nonce"));
        assert!(tab.provisional_launch.is_none());
        assert_eq!(
            tab.restorable_launch.model.as_deref(),
            Some("vendor/next-model")
        );
        tab.pty_id = Some("new-pty".into());
        tab.session_binding_nonce = Some("new-nonce".into());
        tab.agent_booting = false;
        apply_boot_stage(tab, BootStage::ProviderReady, None);
    }
    let attempt = LaunchAttempt::current(&fixture.service.mux, &snap.tab_id, "new-pty").unwrap();
    fixture
        .service
        .record_first_turn_input_ready(&attempt, "opaque replacement composer evidence");
    let current = fixture.service.user_send_authority(&snap.tab_id).unwrap();
    assert!(!fixture
        .service
        .claim_lazy_first_turn_input(&authority, "old-human-send"));
    assert!(fixture
        .service
        .claim_lazy_first_turn_input(&current, "new-human-send"));
    assert!(!fixture
        .service
        .claim_lazy_first_turn_input(&current, "another-human-send"));
}

#[test]
fn final_switch_admission_refuses_replaced_nonce_pty_or_bound_session_without_effects() {
    for change in 0..3 {
        let (fixture, snap, launch, _) = fixture();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&snap.tab_id).unwrap();
            match change {
                0 => tab.session_binding_nonce = Some("replacement-nonce".into()),
                1 => tab.pty_id = Some("replacement-pty".into()),
                _ => tab.agent_session_id = Some("created-session".into()),
            }
        }
        let prior = fixture
            .service
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .unwrap()
            .clone();
        let result = launch.teardown_sessionless_switch(
            &fixture.service,
            &snap,
            || panic!("refused attempt cannot reach teardown"),
            || panic!("refused attempt cannot commit a model"),
        );
        assert!(matches!(
            result,
            Err(SessionlessTeardownError::Admission(_))
        ));
        let tab = fixture
            .service
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .unwrap()
            .clone();
        assert_eq!(tab.pty_id, prior.pty_id);
        assert_eq!(tab.session_binding_nonce, prior.session_binding_nonce);
        assert_eq!(tab.provisional_launch, prior.provisional_launch);
        assert_eq!(tab.agent_booting, prior.agent_booting);
    }
}

#[test]
fn failed_teardown_keeps_old_input_retired_without_model_commit_or_replacement_mutation() {
    for replacement in 0..3 {
        let (fixture, snap, launch, authority) = fixture();
        let result = launch.teardown_sessionless_switch(
            &fixture.service,
            &snap,
            || {
                if replacement != 0 {
                    let mut mux = fixture.service.mux.write();
                    let tab = mux.get_tab_mut(&snap.tab_id).unwrap();
                    if replacement == 1 {
                        tab.session_binding_nonce = Some("replacement-nonce".into());
                    } else {
                        tab.pty_id = Some("replacement-pty".into());
                    }
                    tab.agent_booting = true;
                    apply_boot_stage(tab, BootStage::LaunchWritten, None);
                }
                Err("opaque teardown refusal".into())
            },
            || panic!("failed teardown cannot commit a model"),
        );
        assert!(matches!(result, Err(SessionlessTeardownError::Teardown(_))));
        assert!(!fixture
            .service
            .claim_lazy_first_turn_input(&authority, "old-human-send"));
        let tab = fixture
            .service
            .mux
            .read()
            .get_tab(&snap.tab_id)
            .unwrap()
            .clone();
        assert_eq!(
            tab.restorable_launch.model.as_deref(),
            Some("vendor/prior-model")
        );
        assert_ne!(tab.session_binding_nonce.as_deref(), Some("old-nonce"));
        if replacement != 0 {
            if replacement == 1 {
                assert_eq!(tab.pty_id.as_deref(), Some("old-pty"));
                assert_eq!(
                    tab.session_binding_nonce.as_deref(),
                    Some("replacement-nonce")
                );
            } else {
                assert_eq!(tab.pty_id.as_deref(), Some("replacement-pty"));
            }
            assert_eq!(tab.boot_stage, Some(BootStage::LaunchWritten));
            assert!(tab.agent_booting);
        } else {
            assert_eq!(tab.boot_stage, Some(BootStage::LaunchBindingUnconfirmed));
            assert!(!tab.agent_booting);
            assert!(!tab.provisional_launch.unwrap().first_turn_input_ready);
            assert!(tab
                .boot_failure_reason
                .unwrap()
                .contains("opaque teardown refusal"));
        }
    }
}
