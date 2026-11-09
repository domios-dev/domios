use super::*;
use std::{future::Future, sync::Arc, task::Poll, time::Duration};

#[tokio::test]
async fn cancelled_mesh_persistence_keeps_accepted_writes_in_order() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mesh.json");
    let (entered, started) = oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let mut entered = Some(entered);
    let store = Arc::new(FileOwner::start(path, move || {
        if let Some(entered) = entered.take() {
            entered.send(()).unwrap();
            held.recv().unwrap();
        }
    }));
    let first = tokio::spawn({
        let store = store.clone();
        async move { store.save(AccountMeshRecord::default()).await }
    });
    started.await.unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let latest = AccountMeshRecord {
        signed_out: true,
        ..AccountMeshRecord::default()
    };
    let second = store.save(latest);
    tokio::pin!(second);
    std::future::poll_fn(|cx| {
        assert!(second.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .unwrap()
        .unwrap();
    assert!(store.load().await.unwrap().signed_out);
}
