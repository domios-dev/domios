use crate::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufRead, BufReader, Write};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::Duration;

#[test]
fn tree_member_helper() {
    let Some(mode) = std::env::var_os("CODETERM_TREE_TEST_HELPER") else {
        return;
    };
    let _descendant = if mode == "leader" {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tree_tests::tree_member_helper", "--nocapture"])
            .env("CODETERM_TREE_TEST_HELPER", "descendant")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            if line.unwrap().contains("TREE_READY") {
                break;
            }
        }
        println!("DESCENDANT:{}", child.id());
        Some(child)
    } else {
        None
    };
    println!("TREE_READY");
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
fn force_kill_closes_descendant_output_and_reaps_leader() {
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "tree_tests::tree_member_helper", "--nocapture"]);
    command.env("CODETERM_TREE_TEST_HELPER", "leader");
    let mut child = pair.slave.spawn_command(command).unwrap();
    let reader = pair.master.try_clone_reader().unwrap();
    drop(pair.slave);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (eof_tx, eof_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) if line.contains("DESCENDANT:") => {
                    let _ = ready_tx.send(());
                }
                Ok(_) => {}
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("PTY read failed: {error}"),
            }
        }
        eof_tx.send(()).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    child.kill_tree().unwrap();
    let (exit_tx, exit_rx) = mpsc::channel();
    std::thread::spawn(move || {
        exit_tx.send(child.wait()).unwrap();
    });
    assert!(exit_rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .is_ok());
    eof_rx.recv_timeout(Duration::from_secs(10)).unwrap();
}
