use super::*;

#[test]
fn sync_permit_deadline_enters_runtime_before_constructing_timer() {
    std::thread::spawn(|| {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let gate = tokio::sync::Semaphore::new(1);
        let permit = deadline(Duration::from_secs(1), gate.acquire())
            .unwrap()
            .unwrap();
        assert_eq!(gate.available_permits(), 0);
        drop(permit);
        assert_eq!(gate.available_permits(), 1);
    })
    .join()
    .unwrap();
}
