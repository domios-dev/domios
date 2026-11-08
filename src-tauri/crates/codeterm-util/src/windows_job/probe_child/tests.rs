use super::*;
use std::io::{BufRead, BufReader, Read, Write};

#[test]
#[ignore = "subprocess fixture"]
fn probe_child_fixture() {
    let event = unsafe { CreateEventW(null(), 1, 0, null()) };
    assert!(!event.is_null());
    let event = unsafe { OwnedHandle::from_raw_handle(event) };
    println!("\nprobe-child-ready");
    std::io::stdout().flush().unwrap();
    unsafe { WaitForSingleObject(event.as_raw_handle(), INFINITE) };
}

#[test]
fn provider_version_owner_drop_kills_child_and_excludes_unrelated_handles() {
    let (mut unrelated_reader, unrelated_writer) = io::pipe().unwrap();
    assert_ne!(
        unsafe {
            SetHandleInformation(
                unrelated_writer.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        },
        0
    );
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "windows_job::probe_child::tests::probe_child_fixture",
        "--ignored",
        "--nocapture",
    ]);
    let mut owner = ProbeChild::spawn(&mut command).unwrap();
    let process = owner.process.try_clone().unwrap();
    let stdout = owner.stdout.take().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap() == "probe-child-ready" {
                let _ = ready_tx.send(());
            }
        }
    });
    let ready = ready_rx.recv_timeout(Duration::from_secs(5));
    if ready.is_err() {
        drop(owner);
        reader.join().unwrap();
        panic!("probe child did not signal readiness: {ready:?}");
    }
    drop(unrelated_writer);
    let (eof_tx, eof_rx) = std::sync::mpsc::sync_channel(1);
    let unrelated = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = eof_tx.send(unrelated_reader.read_to_end(&mut bytes));
    });
    let unrelated_eof = eof_rx.recv_timeout(Duration::from_secs(5));
    drop(owner);
    let exited = unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) };
    reader.join().unwrap();
    unrelated.join().unwrap();
    assert_eq!(
        exited, WAIT_OBJECT_0,
        "owner drop must leave no running child"
    );
    assert_eq!(
        unrelated_eof.unwrap().unwrap(),
        0,
        "unrelated pipe must close while child runs"
    );
}
