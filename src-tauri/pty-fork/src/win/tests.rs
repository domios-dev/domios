use crate::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
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
        BufReader::new(descendant.stdout.take().unwrap())
            .lines()
            .find(|line| line.as_ref().is_ok_and(|line| line == "HELPER_READY"))
            .expect("descendant readiness before pipe EOF")
            .unwrap();
        let address = std::env::var("CODETERM_JOB_TEST_READY")
            .unwrap()
            .parse()
            .unwrap();
        let mut ready = TcpStream::connect_timeout(&address, Duration::from_secs(10)).unwrap();
        ready
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        ready.write_all(&descendant.id().to_le_bytes()).unwrap();
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

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let ready_worker = std::thread::spawn(move || {
        let result = (|| {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(10)))?;
            let mut pid = [0; 4];
            stream.read_exact(&mut pid)?;
            Ok::<_, std::io::Error>(u32::from_le_bytes(pid))
        })();
        let _ = ready_tx.send(result);
    });
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "win::tests::job_member_helper", "--nocapture"]);
    command.env("CODETERM_JOB_TEST_HELPER", "leader");
    command.env("CODETERM_JOB_TEST_READY", address.to_string());
    let mut child = pair.slave.spawn_command(command).unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (drain_tx, drain_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = drain_tx.send(std::io::copy(&mut reader, &mut std::io::sink()));
    });
    let readiness = ready_rx.recv_timeout(Duration::from_secs(10));
    if !matches!(&readiness, Ok(Ok(_))) {
        let _ = child.kill_tree();
        let _ = TcpStream::connect_timeout(&address, Duration::from_secs(10));
    }
    ready_worker.join().unwrap();
    let pid = readiness
        .expect("leader readiness event before deadline")
        .expect("descendant PID receipt");
    let raw = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    assert!(!raw.is_null());
    let process = unsafe { filedescriptor::OwnedHandle::from_raw_handle(raw as _) };
    assert!(child.contains_process(raw as _).unwrap());
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
    drop(pair);
    assert!(drain_rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .is_ok());
}
