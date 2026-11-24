use super::*;

#[test]
fn pending_global_stop_fences_racing_startup_until_retirement() {
    let fixture =
        crate::test_support::fixture(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    service.set_neighbor_friend_receiver_for_test("friend", Some(true), 1);
    assert!(service.neighbor_friend_receiving_here("friend"));
    service.queue_neighbor_stop_local();
    service.set_neighbor_friend_receiver_for_test("friend", Some(true), 1);
    assert!(!service.neighbor_friend_receiving_here("friend"));
    *service
        .extensions
        .get_or_init(Pending::default)
        .generation
        .lock() = None;
    assert!(service.neighbor_friend_receiving_here("friend"));
}

#[tokio::test]
async fn off_cleanup_drains_projection_then_closes_before_successor_even_when_waiter_is_abandoned()
{
    use crate::service::neighbors::reconciliation::Kind;
    let fixture =
        crate::test_support::fixture(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    let effects = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let log = effects.clone();
    let projection = service
        .neighbor_reconciliation()
        .submit(Kind::Projection, move || async move {
            started.send(()).unwrap();
            held.await.unwrap();
            log.lock().push("projection write");
            Ok(())
        })
        .unwrap();
    running.await.unwrap();
    service.queue_neighbor_stop_local();
    assert!(service.neighbor_stop_local_pending(0));
    assert!(service
        .neighbor_reconciliation()
        .require_readable()
        .is_err());
    let (closing, close_started) = tokio::sync::oneshot::channel();
    let (closed, close_held) = tokio::sync::oneshot::channel();
    let owner = service.clone();
    let log = effects.clone();
    let (waiter_ready, waiter_started) = tokio::sync::oneshot::channel();
    let stop_service = service.clone();
    let waiter = tokio::spawn(async move {
        waiter_ready.send(()).unwrap();
        stop_service
            .own_neighbor_stop_local(move || async move {
                closing.send(()).unwrap();
                close_held.await.unwrap();
                log.lock().push("OFF close");
                *owner
                    .extensions
                    .get_or_init(Pending::default)
                    .generation
                    .lock() = None;
                Ok(())
            })
            .await
    });
    waiter_started.await.unwrap();
    release.send(()).unwrap();
    projection.wait().await.unwrap();
    close_started.await.unwrap();
    assert_eq!(*effects.lock(), ["projection write"]);
    assert!(service.neighbor_stop_local_pending(0));
    assert!(service
        .neighbor_reconciliation()
        .submit(Kind::Selection, || async { Ok(()) })
        .is_err());
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    closed.send(()).unwrap();
    service.neighbor_reconciliation().idle().await;
    assert!(!service.neighbor_stop_local_pending(0));
    let log = effects.clone();
    service
        .neighbor_reconciliation()
        .submit(Kind::Selection, move || async move {
            log.lock().push("successor");
            Ok(())
        })
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        *effects.lock(),
        ["projection write", "OFF close", "successor"]
    );
}

#[tokio::test]
async fn failed_owned_off_cleanup_keeps_pending_and_refuses_new_account_effects() {
    use crate::service::neighbors::reconciliation::Kind;
    let fixture =
        crate::test_support::fixture(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    service.queue_neighbor_stop_local();
    assert!(service
        .own_neighbor_stop_local(|| async { Err("injected local write failure".into()) })
        .await
        .is_err());
    assert!(service.neighbor_stop_local_pending(0));
    assert!(service
        .neighbor_reconciliation()
        .require_readable()
        .is_err());
    assert!(service
        .neighbor_reconciliation()
        .submit(Kind::Selection, || async { Ok(()) })
        .is_err());
    assert!(service
        .neighbor_reconciliation()
        .submit(Kind::Projection, || async { Ok(()) })
        .is_err());
}

#[tokio::test]
async fn signed_out_off_cleanup_drains_before_the_first_empty_neighbor_list() {
    use crate::service::neighbors::{
        runtime::receiver::selection::tests::Credentials, NeighborCommand,
    };
    use codeterm_neighbors::state::Snapshot;

    let fixture =
        crate::test_support::fixture(crate::multiplexer::state::MultiplexerState::new().0);
    let service = &fixture.service;
    let account = Arc::new(codeterm_account::Account::new(
        Arc::new(codeterm_account::adapters::fake::Fake::default()),
        Arc::new(Credentials),
        std::time::Duration::from_secs(1),
    ));
    assert!(service
        .ext::<crate::service::account::AccountCell>()
        .0
        .set(account.clone())
        .is_ok());
    service.select_neighbor_projection_owned("previous-account", 1);
    let signed_out_generation = service.neighbor_history_generation();
    service.queue_neighbor_stop_local();
    service
        .wait_neighbor_stop_local(signed_out_generation)
        .await
        .unwrap();
    assert!(service.neighbor_stop_local_pending(signed_out_generation));
    assert_eq!(
        service.neighbor_reconciliation().require_readable(),
        Err(Error::LocalFence {
            phase: "off_cleanup"
        })
    );

    account.refresh().await.unwrap();
    service.neighbor_identity_changed();
    let generation = service.neighbor_history_generation();
    assert_ne!(generation, signed_out_generation);
    assert!(!service.neighbor_stop_local_pending(signed_out_generation));
    assert!(service
        .drain_neighbor_stop_local(signed_out_generation)
        .await
        .is_err());
    assert!(service.neighbor_stop_local_pending(generation));
    let session = account.session().await.unwrap();
    let uid = &session.identity.uid;
    let login = session.login_generation;
    service.drain_neighbor_stop_local(generation).await.unwrap();
    assert!(!service.neighbor_stop_local_pending(generation));
    assert_eq!(service.neighbor_reconciliation().require_readable(), Ok(()));
    assert!(service.neighbor_projection_read_epoch(uid, login).is_ok());
    service.refresh_neighbor_list().await.unwrap();
    service
        .select_neighbor_projection(uid, login)
        .await
        .unwrap();
    let snapshot = Snapshot::parse(json!({
        "schema_version": 1, "version": 1, "updated_at": 100, "access": "paid", "can_invite": true,
        "friends": [], "incoming": [], "outgoing": [], "own_keys": [],
        "referral_trial": {"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}
    })).unwrap();
    assert!(
        service
            .apply_neighbor_projection(uid, login, snapshot)
            .await
    );
    let list = service
        .neighbor_registry(NeighborCommand::List {})
        .await
        .unwrap();
    assert_eq!(list["friends"], json!([]));
    assert_eq!(list["can_invite"], true);
}
