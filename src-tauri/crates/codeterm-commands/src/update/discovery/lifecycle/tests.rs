use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test(start_paused = true)]
async fn open_daemon_checks_on_start_and_every_thirty_minutes_until_stopped() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (started, mut receipts) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(periodic(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        started.send(()).unwrap();
        std::future::ready(())
    }));
    receipts.recv().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(INTERVAL - std::time::Duration::from_millis(1)).await;
    assert!(receipts.try_recv().is_err());
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    receipts.recv().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    tokio::time::advance(INTERVAL * 4).await;
    receipts.recv().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::advance(INTERVAL).await;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
