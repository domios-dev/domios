use super::*;

#[test]
fn process_split_contract_dropped_observation_closes_duplicate_witness() {
    let file = tempfile::tempfile().unwrap();
    #[cfg(windows)]
    let witness: ChildWitness = file.try_clone().unwrap().into();
    #[cfg(unix)]
    let witness: ChildWitness = file.try_clone().unwrap().into();
    #[cfg(windows)]
    let raw = {
        use std::os::windows::io::AsRawHandle;
        witness.as_raw_handle()
    };
    #[cfg(unix)]
    let raw = {
        use std::os::fd::AsRawFd;
        witness.as_raw_fd()
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    drop(receiver);
    drop(sender.try_send(WorkerSpawn {
        pid: 1,
        executable: "fixture".into(),
        witness: Ok(witness),
    }));
    #[cfg(windows)]
    {
        let mut flags = 0;
        assert_eq!(
            unsafe { windows_sys::Win32::Foundation::GetHandleInformation(raw, &mut flags) },
            0
        );
    }
    #[cfg(unix)]
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    assert!(file.metadata().is_ok());
}

#[test]
fn process_split_contract_unavailable_witness_is_delivered_without_identity_guess() {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .try_send(WorkerSpawn {
            pid: 42,
            executable: "fixture".into(),
            witness: Err(io::Error::other("unavailable")),
        })
        .ok()
        .unwrap();
    assert!(receiver.try_recv().unwrap().witness.is_err());
}
