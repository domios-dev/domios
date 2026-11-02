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
    let _child = std::env::var("CODETERM_INSTALL_HOLDER_TEST_CHILD")
        .ok()
        .map(|images| {
            let mut images = images.split('|');
            let mut command = helper_command(Path::new(images.next().unwrap()));
            command.env_remove("CODETERM_INSTALL_HOLDER_TEST_CHILD");
            if let Some(next) = images.next() {
                command.env("CODETERM_INSTALL_HOLDER_TEST_CHILD", next);
            }
            command.env("CODETERM_INSTALL_HOLDER_TEST_LINGER", "1");
            let mut child = command.spawn().unwrap();
            read_ready(child.stdout.take().unwrap());
            child
        });
    println!("HOLDER_READY");
    std::io::stdout().flush().unwrap();
    if std::env::var_os("CODETERM_INSTALL_HOLDER_TEST_LINGER").is_some() {
        std::thread::park();
    }
    let _ = std::io::stdin().read(&mut [0u8; 1]);
}

struct Helper(Child);
struct TreeCleanup(Option<OwnedProcessTree>);
impl Drop for TreeCleanup {
    fn drop(&mut self) {
        if let Some(tree) = self.0.take() {
            let _ = tree.terminate_remaining(Duration::from_secs(5));
        }
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn helper(path: &Path) -> Helper {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let test_binary = std::env::current_exe().unwrap();
    std::fs::copy(test_binary, path).unwrap();
    start_helper(helper_command(path))
}

fn helper_command(path: &Path) -> Command {
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
    codeterm_util::process_util::no_window_std(&mut command);
    command
}

fn start_helper(mut command: Command) -> Helper {
    let mut helper = Helper(command.spawn().unwrap());
    let stdout = helper.0.stdout.take().unwrap();
    read_ready(stdout);
    helper
}

fn read_ready(stdout: std::process::ChildStdout) {
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
}

#[test]
fn owned_cli_exits_and_install_file_is_replaced_on_first_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("cli/codeterm.exe");
    let mut helper = helper(&image);
    inspect_directory(dir.path(), true).unwrap();
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
    let error = inspect_directory(dir.path(), true).unwrap_err();
    assert!(error.contains(&format!("foreign.exe (PID {})", foreign.0.id())));
    assert!(own.0.try_wait().unwrap().is_none());
    assert!(foreign.0.try_wait().unwrap().is_none());
}

#[test]
fn pinned_server_descendants_are_confirmed_after_the_server_exits() {
    let dir = tempfile::tempdir().unwrap();
    let server_image = dir.path().join("domios-server/domios-server.exe");
    let child_image = dir.path().join("conpty/x64/OpenConsole.exe");
    for path in [&server_image, &child_image] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), path).unwrap();
    }
    let mut command = helper_command(&server_image);
    command.env(
        "CODETERM_INSTALL_HOLDER_TEST_CHILD",
        format!("{}|{}", server_image.display(), child_image.display()),
    );
    let mut server = start_helper(command);
    let table = process_table().unwrap();
    let created = table.get(&server.0.id()).unwrap().created;
    let _cleanup = TreeCleanup(Some(
        OwnedProcessTree::capture(
            (server.0.id(), created),
            &[server_image.clone(), child_image.clone()],
        )
        .unwrap(),
    ));
    let prepared = inspect_directory_with_server(
        dir.path(),
        false,
        Some(&crate::application_host::AttachedServerIdentity {
            pid: server.0.id(),
            created: created.to_string(),
        }),
    )
    .unwrap();
    server.0.stdin.take().unwrap().write_all(b"x").unwrap();
    server.0.wait().unwrap();
    assert!(prepared.tree.wait_for_exit(Duration::ZERO).is_err());
    prepared.confirm_exited_after(Duration::ZERO).unwrap();
    inspect_directory(dir.path(), true).unwrap();
    std::fs::remove_file(&server_image).unwrap();
    std::fs::remove_file(&child_image).unwrap();
}
