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
                serde_json::json!({"tailnet_ip":format!("100.{}", "64.0.9"),"api_port":7685,"expires_at":1000}),
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
    assert_eq!(state.deadline(0, true), Some(220));
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
fn partition_beyond_five_attempts_recovers_without_registry_changes_or_lists() {
    let mut state = Reconciliation::default();
    let mut peer = row("peer");
    peer.membership_expires_at = Some(10000);
    peer.endpoint.as_mut().unwrap().expires_at = 10000;
    state.observe(1, &[peer.clone()]);
    let mut now = 100;
    let restored_at = 1900;
    let mut attempts = 0;
    let mut list_calls = 1;
    let mut linked_at = None;
    while now <= restored_at + 300 {
        assert!(target(&peer, "own", now).is_some());
        assert!(state.dial_due("own", "peer", now));
        attempts += 1;
        if now >= restored_at {
            state.clear_peer("peer");
            linked_at = Some(now);
            break;
        }
        state
            .retries
            .entry("peer".into())
            .or_default()
            .failed(now, &AdmissionRefusal::PeerUnreachable);
        let deadline = state
            .deadline(now, true)
            .expect("transport retries remain armed");
        assert!(deadline > now && deadline <= now + 300);
        now = deadline;
        if state.registry_refresh_due(now) {
            list_calls += 1;
        }
    }
    assert!(attempts > 5);
    assert!(linked_at.is_some_and(|at| at <= restored_at + 300));
    assert_eq!(list_calls, 1);
    assert_eq!(state.deadline(0, true), None);
}

#[test]
fn network_recovery_rearms_peers_without_bypassing_server_retry_after() {
    let mut state = Reconciliation::default();
    state.observe(1, &[row("peer")]);
    for _ in 0..8 {
        state
            .retries
            .entry("peer".into())
            .or_default()
            .failed(100, &AdmissionRefusal::PeerUnreachable);
    }
    assert!(!state.dial_due("own", "peer", 101));
    state.network_changed();
    assert!(state.dial_due("own", "peer", 101));
    state.server_retry_at = 500;
    state.network_changed();
    assert!(!state.dial_due("own", "peer", 499));
    assert_eq!(state.deadline(499, true), Some(500));
    assert!(state.dial_due("own", "peer", 500));
    assert_eq!(state.deadline(500, true), None);
    assert!(!state.registry_refresh_due(500));
}

#[test]
fn cold_start_six_machines_selects_one_initiator_per_pair() {
    let rows: Vec<_> = (0..6)
        .map(|index| row(&format!("installation-{index}")))
        .collect();
    let mut machines: Vec<_> = (0..6).map(|_| Reconciliation::default()).collect();
    let mut attempts = BTreeSet::new();
    for (index, state) in machines.iter_mut().enumerate() {
        state.observe(1, &rows);
        let own = &rows[index].installation_id;
        for peer in rows.iter().filter(|peer| &peer.installation_id != own) {
            if state.dial_due(own, &peer.installation_id, 0) {
                assert!(own < &peer.installation_id);
                assert!(attempts.insert((own.clone(), peer.installation_id.clone())));
            }
        }
    }
    assert_eq!(attempts.len(), 15);
    for (index, state) in machines.iter_mut().enumerate() {
        for peer in rows
            .iter()
            .filter(|peer| peer.installation_id != rows[index].installation_id)
        {
            state.clear_peer(&peer.installation_id);
        }
        assert_eq!(
            state.deadline(0, true),
            None,
            "connected pairs cancel fallback timers"
        );
    }
}

#[test]
fn higher_installation_falls_back_only_after_one_interval() {
    let mut state = Reconciliation::default();
    state.observe(1, &[row("a")]);
    assert!(!state.dial_due("z", "a", 100));
    assert_eq!(state.deadline(0, true), Some(130));
    assert!(!state.dial_due("z", "a", 129));
    assert!(state.dial_due("z", "a", 130));
    state
        .retries
        .entry("a".into())
        .or_default()
        .failed(130, &AdmissionRefusal::PeerUnreachable);
    assert_eq!(state.deadline(0, true), Some(160));
    assert!(state.dial_due("z", "a", 160));
}

#[test]
fn publication_emits_only_changed_pairs_or_registry_content() {
    use codeterm_account::account_mesh::RegistryObservation;
    use codeterm_mesh::account_mesh::protocol::AccountMeshPair;
    let mut publication = Publication::default();
    let mut events = Vec::new();
    publication.update(vec![], None, |event| events.push(event));
    assert!(events.is_empty());
    let mut registry = RegistryObservation {
        observed_at: 100,
        registry_revision: 1,
        rows: vec![row("peer")],
        cursor: None,
    };
    publication.update(vec![], Some(registry.clone()), |event| events.push(event));
    assert_eq!(events, [ACCOUNT_MESH_CHANGED, "mesh-state-changed"]);
    events.clear();
    registry.observed_at += 1;
    for _ in 0..10 {
        publication.update(vec![], Some(registry.clone()), |event| events.push(event));
    }
    assert!(
        events.is_empty(),
        "unchanged lifecycle pushes and read timestamps stay quiet"
    );
    let pair = AccountMeshPair {
        instance_id: "instance".into(),
        installation_id: Some("peer".into()),
        admission: "account".into(),
        online: true,
        name: "Peer".into(),
    };
    publication.update(vec![pair.clone()], Some(registry.clone()), |event| {
        events.push(event)
    });
    assert_eq!(events.len(), 2);
    events.clear();
    publication.update(vec![pair.clone()], Some(registry.clone()), |event| {
        events.push(event)
    });
    assert!(events.is_empty());
    registry.rows[0].metadata_revision += 1;
    publication.update(vec![pair], Some(registry), |event| events.push(event));
    assert_eq!(events.len(), 2);
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
    assert_eq!(state.deadline(0, true), Some(250));
    assert!(!state.registry_refresh_due(249));
    assert!(state.registry_refresh_due(250));
    state.observe(1, &[row("one")]);
    assert_eq!(state.deadline(0, true), Some(250));
    state.registry_retry = None;
    assert_eq!(state.deadline(0, true), Some(130));
}

#[test]
fn incomplete_registry_or_transport_gate_suspends_overdue_peer_timers() {
    let mut state = Reconciliation::default();
    state.observe(1, &[row("peer")]);
    state
        .retries
        .entry("peer".into())
        .or_default()
        .failed(100, &AdmissionRefusal::PeerUnreachable);
    assert_eq!(state.deadline(200, false), None);
    assert_eq!(state.deadline(200, true), Some(130));
    assert!(state.dial_due("own", "peer", 200));
    let mut registry = Retry::default();
    registry.failed(200, &AdmissionRefusal::Offline);
    state.registry_retry = Some(registry);
    assert_eq!(state.deadline(200, false), Some(230));
    assert!(state.registry_refresh_due(230));
}

#[test]
fn connectivity_flaps_preserve_last_non_transport_refusals() {
    let mut state = Reconciliation::default();
    let refusals = [
        ("offline", AdmissionRefusal::Offline),
        ("unreachable", AdmissionRefusal::PeerUnreachable),
        ("permit", AdmissionRefusal::Unauthenticated),
        ("forbidden", AdmissionRefusal::Revoked),
        ("handshake", AdmissionRefusal::handshake("proof rejected")),
        (
            "busy",
            AdmissionRefusal::RateLimited {
                retry_after_seconds: 600,
            },
        ),
    ];
    let rows: Vec<_> = refusals.iter().map(|(id, _)| row(id)).collect();
    state.observe(1, &rows);
    for (id, refusal) in &refusals {
        let retry = state.retries.entry((*id).into()).or_default();
        retry.failed(99, &AdmissionRefusal::Offline);
        retry.failed(100, refusal);
    }
    for now in [101, 102, 103] {
        state.network_changed();
        state.observe(1, &rows);
        assert!(state.dial_due("a", "offline", now));
        assert!(state.dial_due("a", "unreachable", now));
        for id in ["permit", "forbidden", "handshake", "busy"] {
            assert!(state.retries.contains_key(id), "{id}");
            assert!(!state.dial_due("a", id, now), "{id}");
        }
    }
    assert_eq!(state.retries["busy"].retry_at, Some(700));
}

#[tokio::test]
async fn unrelated_service_events_do_not_finish_the_connectivity_wait() {
    use std::future::{poll_fn, Future};
    use std::task::Poll;
    let bus = codeterm_events::EventBroadcaster::new();
    let mut events = bus.subscribe();
    bus.publish(codeterm_events::SystemEvent::TabFocused {
        tab_id: "tab".into(),
    });
    bus.publish(codeterm_events::SystemEvent::AgentStarted {
        tab_id: "agent".into(),
        provider: "test".into(),
    });
    bus.publish_generic("mesh-state-changed", serde_json::Value::Null);
    let mut waiting = Box::pin(connectivity_changed(&mut events));
    assert!(poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx).is_pending())).await);
    bus.publish_generic(CONNECTIVITY_CHANGED, serde_json::Value::Null);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap(),
        Ok(())
    );
}

#[tokio::test]
async fn connectivity_wait_reconciles_after_lag_and_stops_on_closed_bus() {
    let bus = codeterm_events::EventBroadcaster::new();
    let mut source = bus.subscribe();
    bus.publish_generic("unrelated", serde_json::Value::Null);
    let sample = source.recv().await.unwrap();
    let (sender, mut events) = tokio::sync::broadcast::channel(1);
    sender.send(sample.clone()).unwrap();
    sender.send(sample).unwrap();
    assert_eq!(connectivity_changed(&mut events).await, Ok(()));
    drop(sender);
    assert_eq!(connectivity_changed(&mut events).await, Err(()));
}
