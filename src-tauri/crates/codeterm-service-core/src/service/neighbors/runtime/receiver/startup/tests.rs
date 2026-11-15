use super::*;
use crate::service::neighbors::{projection::ActivationTicket, reconciliation::Kind};
use codeterm_neighbors::state::Snapshot;
use tokio::sync::oneshot;

fn snapshot(version: u64) -> Snapshot {
    Snapshot::parse(json!({"schema_version":1,"version":version,"updated_at":1,"access":"paid","can_invite":false,
        "friends":[],"incoming":[],"outgoing":[],"own_keys":[],
        "referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}})).unwrap()
}

async fn fixture() -> (
    crate::service::test_fixture::ServiceFixture,
    ActivationTicket,
) {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture
        .service
        .select_neighbor_projection("owner", 1)
        .await
        .unwrap();
    assert!(
        fixture
            .service
            .apply_neighbor_projection("owner", 1, snapshot(1))
            .await
    );
    let ticket = fixture
        .service
        .neighbor_activation_ticket("owner", 1)
        .unwrap()
        .0;
    (fixture, ticket)
}

#[tokio::test]
async fn preparation_followed_by_refresh_new_projection_or_account_switch_cannot_apply_local_work()
{
    for transition in 0..3 {
        let (fixture, ticket) = fixture().await;
        let service = &fixture.service;
        let (started, running) = oneshot::channel();
        let (release, held) = oneshot::channel();
        let apply = service.activate_prepared_neighbor(
            ticket,
            0,
            async move {
                started.send(()).unwrap();
                held.await.unwrap();
                Ok(())
            },
            |_| async {
                panic!("Retired preparation cannot write settings, close tabs or start listeners")
            },
        );
        let retire = async {
            running.await.unwrap();
            match transition {
                0 => service.refresh_neighbor_list().await.unwrap(),
                1 => assert!(
                    service
                        .apply_neighbor_projection("owner", 1, snapshot(2))
                        .await
                ),
                _ => service
                    .select_neighbor_projection("other", 1)
                    .await
                    .unwrap(),
            }
            release.send(()).unwrap();
        };
        let (result, _) = tokio::join!(apply, retire);
        assert!(result.is_err());
        assert!(fixture.ui.0.lock().is_empty());
        assert!(service.neighbor_cell().listeners.lock().active.is_empty());
    }
}

#[tokio::test]
async fn admitted_local_application_survives_abandoned_waiter_and_drains_before_retirement() {
    let (fixture, ticket) = fixture().await;
    let service = &fixture.service;
    let writes = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let owner = service.clone();
    let log = writes.clone();
    let waiter = tokio::spawn(async move {
        owner
            .activate_prepared_neighbor(ticket, 0, async { Ok(()) }, move |_| async move {
                started.send(()).unwrap();
                held.await.unwrap();
                log.lock().push("local write");
                Ok(())
            })
            .await
    });
    running.await.unwrap();
    let owner = service.clone();
    let log = writes.clone();
    let retirement = service
        .neighbor_reconciliation()
        .submit(Kind::Retirement, move || async move {
            owner.invalidate_neighbor_projection_list();
            log.lock().push("retired");
            Ok(())
        })
        .unwrap();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(writes.lock().is_empty());
    release.send(()).unwrap();
    retirement.wait().await.unwrap();
    assert_eq!(*writes.lock(), ["local write", "retired"]);
}

#[tokio::test]
async fn stale_preparation_error_does_not_clear_successor_receivers_or_emit_cleanup() {
    let (fixture, ticket) = fixture().await;
    let service = &fixture.service;
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let apply = service.activate_prepared_neighbor::<(), _, _, _>(
        ticket,
        0,
        async move {
            started.send(()).unwrap();
            held.await.unwrap();
            Err("injected preparation failure".into())
        },
        |_| async { panic!("Failed preparation cannot apply") },
    );
    let supersede = async {
        running.await.unwrap();
        assert!(
            service
                .apply_neighbor_projection("owner", 1, snapshot(2))
                .await
        );
        service.set_neighbor_friend_receiver_for_test("successor", Some(true), 1);
        release.send(()).unwrap();
    };
    let (result, _) = tokio::join!(apply, supersede);
    assert!(result.is_err());
    assert!(service.neighbor_friend_receiving_here("successor"));
    assert!(fixture.ui.0.lock().is_empty());
}

#[tokio::test]
async fn off_pending_refuses_activation_and_new_account_until_owned_cleanup_finishes() {
    let (fixture, ticket) = fixture().await;
    let service = &fixture.service;
    service.neighbor_reconciliation().stop_pending();
    assert!(service
        .activate_prepared_neighbor(ticket, 0, async { Ok(()) }, |_| async {
            panic!("OFF cannot admit local application")
        })
        .await
        .is_err());
    assert!(service
        .select_neighbor_projection("other", 1)
        .await
        .is_err());
    let (started, running) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let cleanup = service
        .neighbor_reconciliation()
        .submit(Kind::StopLocal, move || async move {
            started.send(()).unwrap();
            held.await.unwrap();
            Ok(())
        })
        .unwrap();
    running.await.unwrap();
    assert!(service
        .select_neighbor_projection("other", 1)
        .await
        .is_err());
    release.send(()).unwrap();
    cleanup.wait().await.unwrap();
    service
        .select_neighbor_projection("other", 1)
        .await
        .unwrap();
}

struct NoCalls;
#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoCalls {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("No network")
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("No authentication")
    }
}

#[tokio::test]
async fn current_owned_application_retains_real_key_receiver_revision_and_gateway_guards() {
    let (fixture, ticket) = fixture().await;
    let owner = fixture.service.clone();
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let registry = FirebaseRegistry {
        calls: Arc::new(NoCalls),
        uid: "owner".into(),
        installation_id: "here".into(),
        machine_label: "Desk".into(),
    };
    let friend = "a".repeat(64);
    let id = friend.clone();
    fixture.service.activate_prepared_neighbor(ticket, 0, async { Ok("ek_current") }, move |key| async move {
        let list = json!({"friends":[{"friendship_id":id,"my_receiver":{"installation_id":"here","enc_key_id":"ek_current","machine_label":"Desk","revision":7,"receiving":true}}]});
        assert!(owner.reconcile_neighbor_friend_receiver_rows(&metadata, &registry, &list, Some("ek_foreign"), 0).await.is_err());
        assert!(!owner.neighbor_friend_receiving_here(&id));
        owner.reconcile_neighbor_friend_receiver_rows(&metadata, &registry, &list, Some(key), 0).await?;
        assert!(owner.require_neighbor_friend_receiver(&id, Some(7)).is_ok());
        assert!(owner.require_neighbor_friend_receiver(&id, Some(6)).is_err());
        assert!(owner.validate_neighbor_gateway("missing-gateway").is_err());
        Ok(())
    }).await.unwrap();
    assert!(fixture.service.neighbor_friend_receiving_here(&friend));
    assert!(fixture
        .service
        .neighbor_cell()
        .listeners
        .lock()
        .active
        .is_empty());
}

#[tokio::test]
async fn closed_or_reassigned_concierge_keeps_local_receivers_during_restart_activation() {
    for reassigned in [false, true] {
        let (fixture, ticket) = fixture().await;
        let service = &fixture.service;
        let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
        let friend = "a".repeat(64);
        let other = "b".repeat(64);
        let root = if reassigned {
            let mut mux = service.mux.write();
            let workspace = mux.snapshot().workspaces[0].id.clone();
            let root = mux.create_tab(&workspace).unwrap();
            let tab = mux.get_tab_mut(&root).unwrap();
            tab.agent_type = Some("opaque-provider".into());
            tab.role = Some(codeterm_types::pty::AgentRole::Worker);
            root
        } else {
            "closed-root".into()
        };
        let mut settings = crate::service::neighbors::storage::Settings::default();
        settings.bindings.insert(friend.clone(), root.clone());
        settings.permissions.insert(friend.clone(), Permission {
            allowed: true,
            machine_access: codeterm_neighbors::permissions::MachineAccess::Full,
            ..Default::default()
        });
        metadata.save_settings(settings.clone()).await.unwrap();
        let owner = service.clone();
        let id = friend.clone();
        let second = other.clone();
        let stored = metadata.clone();
        service.activate_prepared_neighbor(ticket, 0, async { Ok("ek_current") }, move |key| async move {
            let registry = FirebaseRegistry { calls: Arc::new(NoCalls), uid: "owner".into(), installation_id: "here".into(), machine_label: "Desk".into() };
            let receiver = json!({"installation_id":"here","enc_key_id":"ek_current","machine_label":"Desk","revision":4,"receiving":true});
            let list = json!({"access":"paid","friends":[{"friendship_id":id,"my_receiver":receiver},{"friendship_id":second,"my_receiver":receiver}]});
            owner.reconcile_neighbor_friend_receiver_rows(&stored, &registry, &list, Some(key), 0).await?;
            for friendship in [&id, &second] {
                owner.reconcile_neighbor_concierge_receiver(&stored, "owner", friendship, true, 4, 0).await?;
            }
            Ok(())
        }).await.unwrap();
        let (receivers, _) = service.neighbor_friend_receiver_snapshot();
        assert_eq!(receivers.len(), 2);
        for id in [&friend, &other] {
            assert!(receivers[id].receiving_here);
            assert_eq!(receivers[id].revision, 4);
        }
        let saved = metadata.settings().await.unwrap();
        let projection = service.neighbor_concierge_view(&friend, &saved, &service.neighbor_friend_receiver(&friend).unwrap());
        assert_eq!(projection["state"], if reassigned { "concierge_invalid" } else { "concierge_closed" });
        assert_eq!(saved.bindings, settings.bindings);
        assert_eq!(serde_json::to_value(&saved.permissions).unwrap(), serde_json::to_value(&settings.permissions).unwrap());
        assert!(saved.dormant_concierges.is_empty());
        assert!(service.require_neighbor_restore_grant(&root, false).is_err());
        assert_eq!(service.mux.read().get_tab(&root).is_some(), reassigned);
        assert!(!fixture.ui.0.lock().iter().any(|(name, payload)| name == "neighbor-runtime-status" && payload["receiver_unavailable"] == true));
    }
}
