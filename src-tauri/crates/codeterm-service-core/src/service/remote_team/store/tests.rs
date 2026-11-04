use super::*;
use std::{sync::mpsc, time::Duration};

#[test]
fn held_remote_team_writer_keeps_committed_snapshots_available() {
    let store = Arc::new(RemoteTeamStore::default());
    let (entered, started) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let writer = {
        let store = store.clone();
        std::thread::spawn(move || {
            store.update_with(
                |next| {
                    next.closing_teams.insert("pending".into());
                    next.outgoing.insert("transfer".into(), transfer());
                    Ok(())
                },
                |_| {
                    entered.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                },
            )
        })
    };
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    let (read, result) = mpsc::channel();
    let reader = {
        let store = store.clone();
        std::thread::spawn(move || {
            read.send((
                store.snapshot(),
                store.ensure_tab_lifecycle_available("other"),
                store.ensure_tab_lifecycle_available("parent"),
            ))
            .unwrap()
        })
    };
    let observed = result.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    writer.join().unwrap().unwrap();
    reader.join().unwrap();
    let (snapshot, available, protected) =
        observed.expect("reads must finish while the writer is held");
    assert!(snapshot.unwrap().closing_teams.is_empty());
    available.unwrap();
    assert!(protected.is_err());
    assert!(store.snapshot().unwrap().closing_teams.contains("pending"));
}

fn transfer() -> OutgoingTransfer {
    use crate::service::remote_worker_transfer::RemoteWorkerTransfer;
    OutgoingTransfer {
        operation_id: "transfer".into(),
        peer_id: "peer".into(),
        request: RemoteWorkerTransfer {
            operation_id: "transfer".into(),
            worker_tab_id: "worker".into(),
            worker_session_id: "session".into(),
            worker_provider: "provider".into(),
            expected_parent_tab_id: "parent".into(),
            destination_tab_id: "new-parent".into(),
            destination_team_id: None,
            caller_tab_id: None,
            owner_confirmed: false,
        },
        member: RemoteTeamMember {
            peer_id: "peer".into(),
            tab_id: "worker".into(),
            session_id: "session".into(),
            title: "Worker".into(),
            provider: "provider".into(),
            orchestrator_tab_id: "parent".into(),
            team_id: "team".into(),
            workspace_id: "workspace".into(),
            operation_id: "transfer".into(),
            finalized: false,
        },
        actor_tab_id: None,
        actor_session_id: None,
        completed: false,
    }
}

#[test]
fn failed_remote_team_persistence_does_not_publish_or_keep_tentative_admission() {
    let store = RemoteTeamStore::default();
    store.fail_next_persist();
    assert!(store
        .update(|next| {
            next.outgoing.insert("transfer".into(), transfer());
            Ok(())
        })
        .is_err());
    assert!(store.snapshot().unwrap().outgoing.is_empty());
    store.ensure_tab_lifecycle_available("parent").unwrap();
}
