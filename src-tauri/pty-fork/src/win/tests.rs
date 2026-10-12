use crate::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::{mpsc, Condvar, Mutex};
use std::time::Duration;

#[test]
fn job_member_helper() {
    let Some(mode) = std::env::var_os("CODETERM_JOB_TEST_HELPER") else {
        return;
    };
    let _descendant = if mode == "leader" {
        let mut descendant = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "win::tests::job_member_helper", "--nocapture"])
            .env("CODETERM_JOB_TEST_HELPER", "descendant")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        for line in BufReader::new(descendant.stdout.take().unwrap()).lines() {
            if line.unwrap().contains("HELPER_READY") {
                break;
            }
        }
        println!("DESCENDANT:{}", descendant.id());
        Some(descendant)
    } else {
        None
    };
    println!("HELPER_READY");
    std::io::stdout().flush().unwrap();
    let state = Mutex::new(true);
    let changed = Condvar::new();
    drop(
        changed
            .wait_while(state.lock().unwrap(), |blocked| *blocked)
            .unwrap(),
    );
}

#[test]
fn killed_job_completes_on_process_exit_without_job_time_limit() {
    assert_job_exit(false);
}

#[test]
fn cloned_killer_preserves_job_exit_boundary() {
    assert_job_exit(true);
}

fn assert_job_exit(cloned: bool) {
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::synchapi::WaitForSingleObject;
    use winapi::um::winbase::WAIT_OBJECT_0;
    use winapi::um::winnt::SYNCHRONIZE;

    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "win::tests::job_member_helper", "--nocapture"]);
    command.env("CODETERM_JOB_TEST_HELPER", "leader");
    let mut child = pair.slave.spawn_command(command).unwrap();
    let reader = pair.master.try_clone_reader().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let line = line.unwrap();
            if let Some(pid) = line.trim().strip_prefix("DESCENDANT:") {
                ready_tx.send(pid.parse::<u32>().unwrap()).unwrap();
                return;
            }
        }
    });
    let pid = ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let raw = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    assert!(!raw.is_null());
    let process = unsafe { filedescriptor::OwnedHandle::from_raw_handle(raw as _) };
    if cloned {
        child.clone_killer().kill_tree().unwrap();
    } else {
        child.kill_tree().unwrap();
    }
    let (exit_tx, exit_rx) = mpsc::channel();
    std::thread::spawn(move || {
        exit_tx.send(child.wait()).unwrap();
    });
    assert!(exit_rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .is_ok());
    assert_eq!(unsafe { WaitForSingleObject(raw, 0) }, WAIT_OBJECT_0);
    drop(process);
}
