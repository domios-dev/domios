use super::*;
use crate::types::MeshIdentity;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn make_mesh() -> MeshState {
    Arc::new(Mutex::new(crate::MeshManager::in_memory(
        MeshIdentity {
            instance_id: "local".into(),
            private_key: None,
            public_key: None,
        },
        "Local".into(),
        7685,
    )))
}

#[tokio::test]
async fn burst_changes_coalesce_to_one_fanout_of_the_latest_snapshot() {
    let mesh = make_mesh();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut first = Box::pin(publish_with(&mesh, |_, snapshot| {
        calls.fetch_add(1, Ordering::SeqCst);
        observed.lock().push(snapshot.instance_name);
        Box::pin(async {
            AnnounceReport {
                sent: 3,
                ok: 3,
                ..Default::default()
            }
        })
    }));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(first.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    mesh.lock().ident.instance_name = "Latest".into();
    let queued = publish_with(&mesh, |_, _| panic!("coalesced caller cannot send")).await;
    assert_eq!(queued.sent, 0);
    let report = first.await;
    assert_eq!(report.sent, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*observed.lock(), vec!["Latest"]);
    assert_eq!(
        publish_with(&mesh, |_, _| panic!("same snapshot"))
            .await
            .sent,
        0
    );
}

#[tokio::test]
async fn changes_during_a_pending_wave_deliver_only_one_followup_snapshot() {
    let mesh = make_mesh();
    let calls = Arc::new(AtomicUsize::new(0));
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = tokio::sync::oneshot::channel();
    let started = Arc::new(Mutex::new(Some(started)));
    let blocked = Arc::new(Mutex::new(Some(blocked)));
    let first = {
        let mesh = mesh.clone();
        let calls = calls.clone();
        tokio::spawn(async move {
            publish_with(&mesh, move |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                let started = started.lock().take();
                let blocked = blocked.lock().take();
                Box::pin(async move {
                    if let Some(started) = started {
                        started.send(()).unwrap();
                    }
                    if let Some(blocked) = blocked {
                        bounded(blocked).await.unwrap();
                    }
                    AnnounceReport {
                        sent: 1,
                        ok: 1,
                        ..Default::default()
                    }
                })
            })
            .await
        })
    };
    bounded(ready).await.unwrap();
    for name in ["Intermediate", "Final"] {
        mesh.lock().ident.instance_name = name.into();
        publish_with(&mesh, |_, _| panic!("already running")).await;
    }
    release.send(()).unwrap();
    assert_eq!(bounded(first).await.unwrap().sent, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancellation_releases_the_publisher_for_the_next_change() {
    let mesh = make_mesh();
    let (started, ready) = tokio::sync::oneshot::channel();
    let started = Arc::new(Mutex::new(Some(started)));
    let owner = {
        let mesh = mesh.clone();
        tokio::spawn(async move {
            publish_with(&mesh, move |_, _| {
                let started = started.lock().take();
                Box::pin(async move {
                    if let Some(started) = started {
                        started.send(()).unwrap();
                    }
                    std::future::pending().await
                })
            })
            .await
        })
    };
    bounded(ready).await.unwrap();
    owner.abort();
    assert!(bounded(owner).await.unwrap_err().is_cancelled());
    assert_eq!(
        publish_with(&mesh, |_, _| Box::pin(async {
            AnnounceReport {
                sent: 1,
                ok: 1,
                ..Default::default()
            }
        }))
        .await
        .sent,
        1
    );
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), future)
        .await
        .expect("fixture event must complete")
}
