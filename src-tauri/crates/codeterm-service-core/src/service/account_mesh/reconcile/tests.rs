use super::*;
use codeterm_account::account_mesh::wire::PublicationState;

fn row(id: &str) -> MeshManagementRow {
    MeshManagementRow {
        installation_id: id.into(),
        key_id: "key".into(),
        key_generation: 1,
        slot_revision: 1,
        active: true,
        presence: RegistryPresence::Present,
        admitted_at: 0,
        authenticated_at: 0,
        display_name: Some("same name".into()),
        metadata_revision: 1,
        publication: PublicationState::Complete,
        membership_expires_at: Some(1000),
        eligibility: Eligibility::Eligible,
        endpoint: Some(
            serde_json::from_value(
                serde_json::json!({"tailnet_ip":"100.64.0.9","api_port":7685,"expires_at":1000}),
            )
            .unwrap(),
        ),
    }
}

#[test]
fn registry_targets_require_active_current_present_membership_and_endpoint() {
    let valid = row("peer");
    assert!(target(&valid, "self", 100).is_some());
    assert!(target(&valid, "peer", 100).is_none());
    assert!(target(&valid, "self", 1000).is_none());
    for invalid in [
        MeshManagementRow {
            active: false,
            ..valid.clone()
        },
        MeshManagementRow {
            presence: RegistryPresence::SignedOut,
            ..valid.clone()
        },
        MeshManagementRow {
            eligibility: Eligibility::Revoked,
            ..valid.clone()
        },
        MeshManagementRow {
            endpoint: None,
            ..valid.clone()
        },
        MeshManagementRow {
            membership_expires_at: Some(99),
            ..valid.clone()
        },
    ] {
        assert!(target(&invalid, "self", 100).is_none());
    }
}

#[test]
fn six_installations_resolve_by_identity_despite_equal_names() {
    let rows: Vec<_> = (0..6)
        .map(|index| row(&format!("installation-{index}")))
        .collect();
    let selected: BTreeSet<_> = rows
        .iter()
        .filter_map(|row| target(row, "installation-0", 100))
        .map(|target| target.installation_id)
        .collect();
    assert_eq!(selected.len(), 5);
    assert!(!selected.contains("installation-0"));
}

#[test]
fn rate_limit_blocks_the_whole_wave_until_its_deadline() {
    let mut state = Reconciliation::default();
    state.observe(1, &[row("one"), row("two")]);
    let mut retry = Retry::default();
    retry.failed(
        100,
        &AdmissionRefusal::RateLimited {
            retry_after_seconds: 120,
        },
    );
    state.retries.insert("one".into(), retry);
    state.server_retry_at = 220;
    assert!(!state.due("one", 219));
    assert!(!state.due("two", 219));
    assert_eq!(state.deadline(), Some(220));
    state.observe(1, &[row("one"), row("two")]);
    assert!(!state.due("two", 219));
    assert!(state.due("one", 220));
}

#[test]
fn unrelated_registry_rows_do_not_reset_failed_peer_backoff() {
    let mut state = Reconciliation::default();
    state.observe(1, &[row("one"), row("two")]);
    let mut retry = Retry::default();
    retry.failed(100, &AdmissionRefusal::PeerUnreachable);
    state.retries.insert("one".into(), retry);
    let mut other = row("two");
    other.metadata_revision += 1;
    state.observe(1, &[row("one"), other]);
    assert!(!state.due("one", 101));
    assert!(state.due("two", 101));
    state.observe(2, &[row("one")]);
    assert!(state.due("one", 101));
}

#[test]
fn repeated_transport_failure_parks_after_five_attempts_until_new_evidence() {
    let mut retry = Retry::default();
    for _ in 0..6 {
        retry.failed(100, &AdmissionRefusal::PeerUnreachable);
    }
    assert_eq!(retry.retry_at, None);
    let mut state = Reconciliation::default();
    state.observe(1, &[row("one")]);
    state.retries.insert("one".into(), retry);
    state.observe(1, &[row("one")]);
    assert!(!state.due("one", 10000));
    let mut renewed = row("one");
    renewed.slot_revision += 1;
    state.observe(1, &[renewed]);
    assert!(state.due("one", 10000));
}

#[test]
fn registry_outage_uses_one_retry_deadline_and_blocks_past_peer_deadlines() {
    let mut state = Reconciliation::default();
    state.observe(1, &[row("one")]);
    let mut peer = Retry::default();
    peer.failed(100, &AdmissionRefusal::PeerUnreachable);
    state.retries.insert("one".into(), peer);
    let mut registry = Retry::default();
    registry.failed(
        130,
        &AdmissionRefusal::RateLimited {
            retry_after_seconds: 120,
        },
    );
    state.registry_retry = Some(registry);
    assert_eq!(state.deadline(), Some(250));
    state.observe(1, &[row("one")]);
    assert_eq!(state.deadline(), Some(250));
    state.registry_retry = None;
    assert_eq!(state.deadline(), Some(130));
}
