use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn holder_test_helper() {
    if std::env::var_os("CODETERM_INSTALL_HOLDER_TEST_HELPER").is_none() {
        return;
    }
    println!("HOLDER_READY");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0u8; 1]);
}

struct Helper(Child);
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn helper(path: &Path) -> Helper {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), path).unwrap();
    let mut command = Command::new(path);
    command
        .args([
            "--exact",
            "update::install_holders::windows::tests::holder_test_helper",
            "--nocapture",
        ])
        .env("CODETERM_INSTALL_HOLDER_TEST_HELPER", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    codeterm_service_core::process_util::no_window_std(&mut command);
    let mut helper = Helper(command.spawn().unwrap());
    let stdout = helper.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.unwrap().contains("HOLDER_READY") {
                tx.send(()).unwrap();
                return;
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    helper
}

#[test]
fn owned_cli_exits_and_install_file_is_replaced_on_first_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("cli/codeterm.exe");
    let mut helper = helper(&image);
    inspect_directory(dir.path(), true, None).unwrap();
    helper.0.wait().unwrap();
    let replacement = dir.path().join("replacement.exe");
    std::fs::write(&replacement, b"replacement").unwrap();
    std::fs::remove_file(&image).unwrap();
    std::fs::rename(&replacement, &image).unwrap();
    assert_eq!(std::fs::read(&image).unwrap(), b"replacement");
}

#[test]
fn foreign_holder_is_named_and_neither_it_nor_owned_cli_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let mut own = helper(&dir.path().join("cli/codeterm.exe"));
    let mut foreign = helper(&dir.path().join("foreign.exe"));
    let error = inspect_directory(dir.path(), true, None).unwrap_err();
    assert!(error.contains(&format!("foreign.exe (PID {})", foreign.0.id())));
    assert!(own.0.try_wait().unwrap().is_none());
    assert!(foreign.0.try_wait().unwrap().is_none());
}
