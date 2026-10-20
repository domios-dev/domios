use super::claim_restored_pty_boot;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn fixture(owners: &[&str]) -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.protected = false;
    tab.restored_from_snapshot = true;
    tab.agent_type = None;
    tab.agent_session_id = Some("retained-session".into());
    tab.cwd = Some("D:/fixture".into());
    let fixture = ServiceFixture::new(mux);
    for owner in owners {
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(MockProvider::with_session(
                owner,
                "retained-session",
            )));
    }
    (fixture, tab_id)
}

fn journal_provider_for_tab(fixture: &ServiceFixture, tab_id: &str, provider: &str) {
    use crate::agent::provider_identity::ProviderIdentityEvidence;
    use crate::session::lifecycle::{AgentTypeWriteOrigin, LifecycleTransition};

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.session_binding_nonce = Some("retained-nonce".into());
        tab.reported_session_binding_nonce = Some("retained-nonce".into());
    }
    fixture
        .service
        .open_lifecycle_epoch(tab_id)
        .expect("the restored PTY tab opens a journal epoch");
    let evidence = ProviderIdentityEvidence::capture(
        fixture
            .service
            .mux
            .read()
            .get_tab(tab_id)
            .expect("fixture tab exists"),
    );
    let committed = fixture
        .service
        .commit_verified_tab_agent_type_with_origin(
            tab_id,
            provider.into(),
            AgentTypeWriteOrigin::Repair,
            None,
            &evidence,
            |_| Ok::<_, ()>(()),
        )
        .expect("provider authority write succeeds");
    assert!(matches!(
        committed,
        Some(crate::service::provider_identity::ConditionalAgentTypeWrite::Committed { .. })
    ));
    fixture
        .service
        .record_lifecycle(tab_id, LifecycleTransition::LaunchReady);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .agent_type = None;
}

#[test]
fn legacy_restored_provider_recovers_from_its_sealed_tab_session_checkpoint() {
    let (fixture, tab_id) = fixture(&[]);
    journal_provider_for_tab(&fixture, &tab_id, "journal-owner");
    fixture
        .service
        .recover_missing_restored_provider(&tab_id)
        .unwrap();
    fixture
        .service
        .recover_missing_restored_provider(&tab_id)
        .unwrap();
    let claim = claim_restored_pty_boot(&fixture.service.mux, &tab_id)
        .unwrap()
        .unwrap();
    assert_eq!(claim.provider_id, "journal-owner");
    assert_eq!(claim.session_id.as_deref(), Some("retained-session"));
    assert!(claim_restored_pty_boot(&fixture.service.mux, &tab_id)
        .unwrap()
        .is_none());
}

#[test]
fn missing_restored_provider_surfaces_a_reason_without_unique_journal_provenance() {
    for owners in [vec![], vec!["owner-one"], vec!["owner-one", "owner-two"]] {
        let (fixture, tab_id) = fixture(&owners);
        let claim = fixture.service.pty_ownership.acquire(&tab_id);
        let error = fixture
            .service
            .materialize_restored_agent_with_claim(&claim)
            .unwrap_err();
        assert!(error.starts_with("restored_provider_unresolved:"));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).expect("unresolved tab is retained");
        assert!(tab.agent_type.is_none());
        assert_eq!(tab.agent_session_id.as_deref(), Some("retained-session"));
        assert_eq!(
            tab.boot_stage,
            Some(crate::service::boot_stage::BootStage::SpawnFailed)
        );
        assert_eq!(tab.boot_failure_reason.as_deref(), Some(error.as_str()));
    }
}

#[test]
fn missing_restored_provider_does_not_revive_a_non_restored_or_booting_tab() {
    for booting in [false, true] {
        let (fixture, tab_id) = fixture(&["fixture-owner"]);
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.restored_from_snapshot = booting;
            tab.agent_booting = booting;
        }
        fixture
            .service
            .recover_missing_restored_provider(&tab_id)
            .unwrap();
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_type
            .is_none());
    }
}
