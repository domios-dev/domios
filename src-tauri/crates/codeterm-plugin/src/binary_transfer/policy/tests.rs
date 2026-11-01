use super::*;
use crate::binary_transfer::tests::fixture;
#[test]
fn selected_ticket_expiry_and_capacity_are_enforced_without_waiting() {
    let (directory, session, _, _) = fixture(vec![]);
    let path = directory.path().join("selected");
    std::fs::write(&path, [0, 255]).unwrap();
    let mut handles = Vec::new();
    for _ in 0..8 {
        handles.push(
            session
                .select_input(
                    "https://api.example",
                    File::open(&path).unwrap(),
                    "input.bin",
                    "application/octet-stream",
                    None,
                )
                .unwrap()
                .handle,
        );
    }
    assert_eq!(
        session
            .select_input(
                "https://api.example",
                File::open(&path).unwrap(),
                "input.bin",
                "application/octet-stream",
                None
            )
            .unwrap_err(),
        TransferRefusal::Unavailable
    );
    session
        .selected
        .lock()
        .get_mut(&handles[0])
        .unwrap()
        .expires = Instant::now();
    assert!(session
        .take_input(&handles[0], &Url::parse("https://api.example/").unwrap())
        .is_err());
    let fresh = session
        .select_input(
            "https://api.example",
            File::open(&path).unwrap(),
            "input.bin",
            "application/octet-stream",
            None,
        )
        .unwrap();
    assert_ne!(fresh.handle, handles[0]);
    assert_eq!(session.selected.lock().len(), 8);
}
#[test]
fn preparation_checks_abort_and_deadline_before_minting_a_ticket() {
    let (directory, session, _, _) = fixture(vec![]);
    let path = directory.path().join("selected");
    std::fs::write(&path, [0, 255]).unwrap();
    let abort = TransferAbort::default();
    abort.abort();
    let control = InputPreparation {
        abort,
        deadline: Instant::now() + Duration::from_secs(2),
    };
    assert_eq!(
        session
            .select_input_bounded(
                "https://api.example",
                File::open(&path).unwrap(),
                "input.bin",
                "application/octet-stream",
                None,
                &control
            )
            .unwrap_err(),
        TransferRefusal::Aborted
    );
    let control = InputPreparation {
        abort: TransferAbort::default(),
        deadline: Instant::now(),
    };
    assert_eq!(
        session
            .select_input_bounded(
                "https://api.example",
                File::open(&path).unwrap(),
                "input.bin",
                "application/octet-stream",
                None,
                &control
            )
            .unwrap_err(),
        TransferRefusal::Deadline
    );
    assert!(session.selected.lock().is_empty());
}
