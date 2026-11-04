use super::*;
use std::{io::Read, path::Path};

#[test]
fn windows_denies_unc_drive_relative_ads_and_device_aliases() {
    let root = Path::new("C:/workspace");
    for path in [
        "//server/share/file",
        "\\\\server\\share\\file",
        "C:file",
        "C:/workspace/file:stream",
        "C:/workspace/CON",
        "C:/workspace/a.",
        "C:/workspace/../file",
        "C:/workspace-other/file",
    ] {
        assert!(fs::relative(root, path).is_err(), "{path}");
    }
    assert_eq!(
        fs::relative(root, "C:/workspace/file").unwrap(),
        Path::new("file")
    );
}

#[test]
fn windows_denies_all_reparse_components_and_nonregular_files() {
    let (_temp, root) = tests::root();
    let (_outside, outside) = tests::root();
    std::fs::write(root.join("regular"), "inside").unwrap();
    std::fs::write(outside.join("regular"), "outside").unwrap();
    std::os::windows::fs::symlink_file(root.join("regular"), root.join("internal-link")).unwrap();
    std::os::windows::fs::symlink_dir(&outside, root.join("external-link")).unwrap();
    std::os::windows::fs::symlink_dir(&root, root.join("self-link")).unwrap();
    std::fs::create_dir(root.join("directory")).unwrap();
    let handle = fs::windows::open_root(&root).unwrap();
    for path in [
        "internal-link",
        "external-link/regular",
        "self-link/regular",
        "directory",
    ] {
        assert!(fs::read(&root, &handle, Path::new(path), None, None).is_err());
        assert!(fs::write(&root, &handle, Path::new(path), "changed", |apply| apply()).is_err());
    }
    assert!(fs::windows::open_root(&root.join("self-link")).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.join("regular")).unwrap(),
        "outside"
    );
    assert_eq!(
        fs::read(&root, &handle, Path::new("regular"), None, None).unwrap(),
        "inside"
    );
}

#[test]
fn windows_atomic_commit_pins_parent_and_rechecks_leaf() {
    let (_temp, root) = tests::root();
    std::fs::create_dir(root.join("parent")).unwrap();
    std::fs::write(root.join("parent/file"), "old").unwrap();
    let handle = fs::windows::open_root(&root).unwrap();
    assert!(
        fs::write(&root, &handle, Path::new("parent/file"), "new", |_apply| {
            Err("expired grant".into())
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("parent/file")).unwrap(),
        "old"
    );
    assert_eq!(std::fs::read_dir(root.join("parent")).unwrap().count(), 1);
    assert!(
        fs::write(&root, &handle, Path::new("parent/file"), "new", |apply| {
            assert!(std::fs::rename(root.join("parent"), root.join("moved")).is_err());
            std::fs::remove_file(root.join("parent/file")).unwrap();
            std::os::windows::fs::symlink_file(root.join("target"), root.join("parent/file"))
                .unwrap();
            apply()
        })
        .is_err()
    );
    assert!(!root.join("target").exists());
    std::fs::remove_file(root.join("parent/file")).unwrap();
    std::fs::write(root.join("parent/file"), "old").unwrap();
    let mut old = std::fs::File::open(root.join("parent/file")).unwrap();
    fs::write(&root, &handle, Path::new("parent/file"), "new", |apply| {
        apply()
    })
    .unwrap();
    let mut previous = String::new();
    old.read_to_string(&mut previous).unwrap();
    assert_eq!(previous, "old");
    assert_eq!(
        std::fs::read_to_string(root.join("parent/file")).unwrap(),
        "new"
    );
    assert_eq!(std::fs::read_dir(root.join("parent")).unwrap().count(), 1);
}

#[test]
fn windows_terminal_refuses_batch_shell_dispatch() {
    let (_temp, root) = tests::root();
    for command in ["script.cmd", "script.BAT", "relative.exe"] {
        let spec = terminal::Command {
            command: command.into(),
            args: vec!["& unexpected".into()],
            cwd: None,
            env: vec![],
            output_byte_limit: None,
        };
        assert!(terminal::Terminal::spawn(
            &spec,
            &root,
            fs::windows::open_root(&root).unwrap(),
            &BTreeMap::new()
        )
        .is_err());
    }
}

#[test]
fn windows_install_requires_an_available_root_and_declared_bridges() {
    let (_temp, root) = tests::root();
    let context = tests::context(&root);
    assert_eq!(install(&context, None).unwrap(), None);
    assert!(bridge(&context).is_err());
    assert_eq!(
        install(&context, Some(tests::capabilities())).unwrap(),
        Some(tests::capabilities())
    );
    BRIDGES.lock().remove(&context.tab_id);
}

#[test]
#[ignore = "Child process fixture launched only by the terminal lifecycle tests"]
fn windows_terminal_process_fixture() {
    use std::io::Write;
    if std::env::var_os("DOMIOS_BRIDGE_CHILD_FIXTURE").is_some() {
        loop {
            std::thread::park();
        }
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "service::agent_runtime::client_bridge::windows_tests::windows_terminal_process_fixture", "--ignored", "--nocapture", "--test-threads=1"])
        .env("DOMIOS_BRIDGE_CHILD_FIXTURE", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn().unwrap();
    std::fs::write("child.pid", child.id().to_string()).unwrap();
    std::io::stdout()
        .write_all(&vec![b'x'; fs::MAX_BYTES + 8192])
        .unwrap();
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

pub(super) fn retain_descendant(root: &Path) -> std::os::windows::io::OwnedHandle {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    let pid = std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null());
    unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) }
}

pub(super) fn assert_descendant_exited(handle: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
}

#[test]
fn windows_terminal_handle_bound_refuses_ninth_and_release_reclaims_slots() {
    crate::async_runtime::block_on(async {
        let (_temp, root) = tests::root();
        let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
        let session = engine::for_tab(&tab).unwrap();
        session.context.lock().cwd = root.to_string_lossy().into_owned();
        let context = session.context.lock().clone();
        install(&context, Some(tests::capabilities())).unwrap();
        let bridge = bridge(&context).unwrap();
        let session_id = context.thread_id.clone().unwrap();
        assert!(execute(
            &fixture.service,
            &session,
            &context,
            &bridge,
            0,
            Request::Output {
                session_id: session_id.clone(),
                terminal_id: "foreign".into()
            },
            BTreeMap::new()
        )
        .await
        .is_err());
        let command = terminal::Command {
            command: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec!["--list".into()],
            cwd: None,
            env: vec![],
            output_byte_limit: Some(32),
        };
        let mut ids = Vec::new();
        for _ in 0..8 {
            let created = execute(
                &fixture.service,
                &session,
                &context,
                &bridge,
                0,
                Request::Create {
                    session_id: session_id.clone(),
                    command: command.clone(),
                },
                BTreeMap::new(),
            )
            .await
            .unwrap();
            ids.push(created["terminalId"].as_str().unwrap().to_owned());
        }
        let refused = execute(
            &fixture.service,
            &session,
            &context,
            &bridge,
            0,
            Request::Create {
                session_id: session_id.clone(),
                command,
            },
            BTreeMap::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(refused, "Terminal capacity unavailable");
        assert_eq!(bridge.state.lock().terminals.len(), 8);
        for terminal_id in ids {
            execute(
                &fixture.service,
                &session,
                &context,
                &bridge,
                0,
                Request::Release {
                    session_id: session_id.clone(),
                    terminal_id,
                },
                BTreeMap::new(),
            )
            .await
            .unwrap();
        }
        assert!(bridge.state.lock().terminals.is_empty());
        let process = session.process.lock().clone().unwrap();
        cancel(&fixture.service, &session, &process, true)
            .await
            .unwrap();
    });
}
