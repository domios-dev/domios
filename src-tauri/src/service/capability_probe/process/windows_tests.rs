use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::{
        CompareObjectHandles, DuplicateHandle, GetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE,
        HANDLE_FLAG_INHERIT,
    },
    Security::SECURITY_ATTRIBUTES,
    System::{
        Pipes::CreatePipe,
        Threading::{GetCurrentProcess, WaitForSingleObject},
    },
};

#[test]
fn capability_probe_spawn_excludes_a_foreign_inheritable_pipe() {
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut reader = std::ptr::null_mut();
    let mut writer = std::ptr::null_mut();
    assert_ne!(
        unsafe { CreatePipe(&mut reader, &mut writer, &security, 0) },
        0
    );
    let _reader = unsafe { OwnedHandle::from_raw_handle(reader.cast()) };
    let writer = unsafe { OwnedHandle::from_raw_handle(writer.cast()) };
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        ready_tx.send(writer.as_raw_handle() as usize).unwrap();
        let _ = done_rx.recv();
        drop(writer);
    });
    let foreign = ready_rx.recv().unwrap() as HANDLE;
    let shell =
        std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("Windows SystemRoot"))
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(shell);
    command
        .env_clear()
        .envs(std::env::vars_os())
        .args(["-NoProfile", "-NonInteractive", "-Command", "exit 0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tree = crate::service::provider_update::ProcessTree::prepare(&mut command).unwrap();
    let mut child = spawn(&mut command).unwrap();
    tree.record_spawn(&child).unwrap();
    let parent_ends_isolated = [
        child.stdin.as_ref().unwrap().as_raw_handle(),
        child.stdout.as_ref().unwrap().as_raw_handle(),
        child.stderr.as_ref().unwrap().as_raw_handle(),
    ]
    .into_iter()
    .all(|handle| {
        let mut flags = 0;
        unsafe {
            GetHandleInformation(handle.cast(), &mut flags) != 0 && flags & HANDLE_FLAG_INHERIT == 0
        }
    });
    let mut copy = std::ptr::null_mut();
    let inherited = unsafe {
        DuplicateHandle(
            child.as_raw_handle().cast(),
            foreign,
            GetCurrentProcess(),
            &mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } != 0;
    let same_object = if inherited {
        let copy = unsafe { OwnedHandle::from_raw_handle(copy.cast()) };
        unsafe { CompareObjectHandles(foreign, copy.as_raw_handle().cast()) != 0 }
    } else {
        false
    };
    tree.attach(&child).unwrap();
    let completed = unsafe { WaitForSingleObject(child.as_raw_handle().cast(), 5000) } == 0;
    if !completed {
        tree.terminate(&mut child).unwrap();
    }
    let status = child.wait().unwrap();
    tree.reaped(&child);
    done_tx.send(()).unwrap();
    holder.join().unwrap();
    assert!(
        !same_object,
        "probe inherited another thread's pipe writer (completed={completed}, status={status:?})"
    );
    assert!(parent_ends_isolated);
    assert!(completed);
    assert!(status.success(), "{status:?}");
}
