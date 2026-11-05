use super::*;
use std::{io::Read, path::Path};

#[test]
fn windows_root_open_accepts_tempdir_and_repository_after_file_denial() {
    let (_temp, root) = tests::root();
    let file = root.join("regular");
    std::fs::write(&file, "not a directory").unwrap();
    assert!(fs::windows::open_root(&file).is_err());
    let ordinary = fs::windows::host_disk_path(&root).unwrap();
    let repository = std::env::current_dir().unwrap();
    for path in [root, ordinary, repository] {
        let handle = fs::windows::open_root(&path)
            .unwrap_or_else(|error| panic!("open directory {}: {error}", path.display()));
        assert!(handle.metadata().unwrap().is_dir());
        fs::root_matches(&path, &handle).unwrap();
    }
}

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
        "C:/workspace/ENV~1",
        "C:/workspace/SSH~1/id_rsa",
    ] {
        assert!(fs::relative(root, path).is_err(), "{path}");
    }
    assert_eq!(
        fs::relative(root, "C:/workspace/file").unwrap(),
        Path::new("file")
    );
}

#[test]
fn windows_resolved_secret_policy_refuses_alias_classification_changes() {
    assert!(fs::windows::same_policy(Path::new("plain"), Path::new(".env")).is_err());
    assert!(fs::windows::same_policy(Path::new("alias/id_rsa"), Path::new(".ssh/id_rsa")).is_err());
    assert!(fs::windows::same_policy(Path::new(".env"), Path::new(".env")).is_ok());
}

#[test]
fn windows_short_names_never_bypass_sensitive_file_approval_or_machine_denial() {
    let (_temp, root) = tests::root();
    std::fs::write(root.join(".env"), "protected").unwrap();
    std::fs::create_dir(root.join(".ssh")).unwrap();
    std::fs::write(root.join(".ssh/id_rsa"), "machine-protected").unwrap();
    let env_alias = short_path(&root.join(".env"))
        .file_name()
        .unwrap()
        .to_owned();
    let ssh_alias = short_path(&root.join(".ssh"))
        .file_name()
        .unwrap()
        .to_owned();
    assert_ne!(
        env_alias, ".env",
        "Windows peer must generate 8.3 aliases for this denial proof"
    );
    assert_ne!(
        ssh_alias, ".ssh",
        "Windows peer must generate 8.3 aliases for this denial proof"
    );
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    {
        let mut context = session.context.lock();
        context.cwd = root.to_string_lossy().into_owned();
        context.active_turn_id = Some("alias-proof-turn".into());
    }
    let context = session.context.lock().clone();
    install(&context, Some(tests::capabilities())).unwrap();
    let installed = bridge(&context).unwrap();
    let root_handle = fs::windows::open_root(&root).unwrap();
    for path in [root.join(env_alias), root.join(ssh_alias).join("id_rsa")] {
        let relative = path.strip_prefix(&root).unwrap();
        assert!(fs::read(&root, &root_handle, relative, None, None).is_err());
        assert!(fs::write(&root, &root_handle, relative, "changed", |apply| apply()).is_err());
        for request in [
            Request::Read {
                session_id: context.thread_id.clone().unwrap(),
                path: path.to_string_lossy().into_owned(),
                line: None,
                limit: None,
            },
            Request::Write {
                session_id: context.thread_id.clone().unwrap(),
                path: path.to_string_lossy().into_owned(),
                content: "changed".into(),
            },
        ] {
            assert!(prepare(&fixture.service, &session, &context, &installed, &request).is_err());
        }
    }
    let request = Request::Read {
        session_id: context.thread_id.clone().unwrap(),
        path: root.join(".env").to_string_lossy().into_owned(),
        line: None,
        limit: None,
    };
    assert!(
        prepare(&fixture.service, &session, &context, &installed, &request)
            .unwrap()
            .0
            .is_some()
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".env")).unwrap(),
        "protected"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".ssh/id_rsa")).unwrap(),
        "machine-protected"
    );
    BRIDGES.lock().remove(&context.tab_id);
}

#[test]
fn windows_component_containment_ignores_case_without_accepting_siblings() {
    assert!(fs::relative(Path::new("C:/Work"), "c:/WORK-other/file").is_err());
    assert_eq!(
        fs::relative(Path::new("C:/Work"), "c:/WORK/file").unwrap(),
        Path::new("file")
    );
}

#[test]
fn windows_installed_bridge_allows_external_atomic_saves() {
    let (_temp, root) = tests::root();
    let context = tests::context(&root);
    install(&context, Some(tests::capabilities())).unwrap();
    assert!(std::fs::rename(&root, root.with_extension("moved")).is_err());
    std::fs::write(root.join("file"), "old").unwrap();
    std::fs::write(root.join("third-party-temporary"), "new").unwrap();
    std::fs::rename(root.join("third-party-temporary"), root.join("file")).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "new");
    let installed = bridge(&context).unwrap();
    let cwd = fs::windows::cwd(&installed.root_handle).unwrap();
    assert!(
        matches!(cwd.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    );
    assert!(
        fs::windows::relative_directory(&cwd, root.to_str().unwrap())
            .unwrap()
            .as_os_str()
            .is_empty()
    );
    BRIDGES.lock().remove(&context.tab_id);
}

#[test]
fn windows_program_resolution_refuses_shells_relative_search_and_cwd_impostors() {
    let (_temp, root) = tests::root();
    let tools = root.join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(root.join("where.exe"), "cwd-impostor").unwrap();
    native_executable(&tools.join("where.exe"));
    std::fs::write(tools.join("batch.cmd"), "shell").unwrap();
    let env = BTreeMap::from([
        ("PATH".into(), format!(";.;relative;{}", tools.display())),
        ("PATHEXT".into(), ".CMD;.BAT".into()),
    ]);
    for command in ["batch", "batch.cmd", "script.BAT", "./where.exe"] {
        assert!(terminal::windows::program(command, &env, false).is_err());
    }
    assert_eq!(
        terminal::windows::program("where", &env, false)
            .unwrap()
            .path
            .canonicalize()
            .unwrap(),
        tools.join("where.exe").canonicalize().unwrap()
    );
    native_executable(&tools.join("utility.com"));
    assert_eq!(
        terminal::windows::program("utility", &env, false)
            .unwrap()
            .path
            .canonicalize()
            .unwrap(),
        tools.join("utility.com").canonicalize().unwrap()
    );
    native_executable(&tools.join("cmd.exe"));
    assert_eq!(
        terminal::windows::program("cmd", &env, false)
            .unwrap()
            .path
            .canonicalize()
            .unwrap(),
        tools.join("cmd.exe").canonicalize().unwrap()
    );
}

#[test]
fn windows_denies_all_reparse_components_and_nonregular_files() {
    let (_temp, root) = tests::root();
    let (_outside, outside) = tests::root();
    std::fs::write(root.join("regular"), "inside").unwrap();
    std::fs::write(outside.join("regular"), "outside").unwrap();
    let reparse_file = guid_file_reparse(&root.join("internal-file"));
    junction(&root.join("internal-link"), &root);
    junction(&root.join("external-link"), &outside);
    junction(&root.join("self-link"), &root);
    std::fs::create_dir(root.join("directory")).unwrap();
    let handle = fs::windows::open_root(&root).unwrap();
    for path in [
        "internal-link",
        "internal-link/regular",
        "external-link/regular",
        "self-link/regular",
        "directory",
    ] {
        assert_eq!(
            bridge_failure(fs::read(&root, &handle, Path::new(path), None, None).unwrap_err()).code,
            -32602,
        );
        assert!(fs::write(&root, &handle, Path::new(path), "changed", |apply| apply()).is_err());
    }
    let error = fs::read(&root, &handle, Path::new("internal-file"), None, None).unwrap_err();
    assert!(
        error.to_string().contains("Reparse attribute"),
        "file read must fail on reparse attribute: {error}"
    );
    let error = fs::write(
        &root,
        &handle,
        Path::new("internal-file"),
        "changed",
        |apply| apply(),
    )
    .unwrap_err();
    assert!(
        error.contains("Reparse attribute"),
        "file write must fail on reparse attribute: {error}"
    );
    clear_guid_reparse(&reparse_file);
    drop(reparse_file);
    assert_eq!(
        std::fs::read_to_string(root.join("internal-file")).unwrap(),
        "ordinary file"
    );
    std::fs::remove_file(root.join("internal-file")).unwrap();
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
fn windows_temporary_file_cannot_be_replaced_or_mutated_before_commit() {
    let (_temp, root) = tests::root();
    let handle = fs::windows::open_root(&root).unwrap();
    fs::write(&root, &handle, Path::new("file"), "approved", |apply| {
        let temporary = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".domios-bridge-")
            })
            .unwrap();
        assert!(std::fs::rename(&temporary, root.join("stolen")).is_err());
        assert!(std::fs::write(&temporary, "substituted").is_err());
        apply()
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "approved"
    );
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn windows_atomic_commit_pins_parent_and_rechecks_leaf() {
    let (_temp, root) = tests::root();
    let (_outside, outside) = tests::root();
    std::fs::write(outside.join("file"), "outside").unwrap();
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
    let mut substituted = None;
    let error = fs::write(&root, &handle, Path::new("parent/file"), "new", |apply| {
        assert!(std::fs::rename(root.join("parent"), root.join("moved")).is_err());
        std::fs::remove_file(root.join("parent/file")).unwrap();
        substituted = Some(guid_file_reparse(&root.join("parent/file")));
        apply()
    })
    .unwrap_err();
    assert!(
        error.contains("Reparse attribute"),
        "commit must fail on reparse attribute: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("file")).unwrap(),
        "outside"
    );
    clear_guid_reparse(substituted.as_ref().unwrap());
    drop(substituted);
    std::fs::remove_file(root.join("parent/file")).unwrap();
    std::fs::write(root.join("parent/file"), "old").unwrap();
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut blocked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(root.join("parent/file"))
        .unwrap();
    let error = fs::write(
        &root,
        &handle,
        Path::new("parent/file"),
        "blocked",
        |apply| apply(),
    )
    .unwrap_err();
    assert_eq!(error, fs::windows::CommitRefusal::FileInUse.to_string());
    assert_eq!(
        std::fs::read_to_string(root.join("parent/file")).unwrap(),
        "old"
    );
    let mut original = String::new();
    blocked.read_to_string(&mut original).unwrap();
    assert_eq!(original, "old");
    assert_eq!(
        std::fs::read_dir(root.join("parent")).unwrap().count(),
        1,
        "failed commit must remove its temporary file"
    );
    drop(blocked);
    let mut old = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(root.join("parent/file"))
        .unwrap();
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
    for command in ["script.cmd", "script.BAT", "./relative.exe"] {
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
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    crate::process_util::no_window_std(&mut command);
    let mut child = command
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
    // The descendant parks until the host's tree kill, so this reaps it without ending the fixture early.
    let _ = child.wait();
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
        assert_eq!(refused, "Terminal capacity unavailable".into());
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

fn short_path(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let input: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut output = vec![0u16; 32768];
    let length =
        unsafe { GetShortPathNameW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32) };
    assert!(length > 0 && (length as usize) < output.len());
    PathBuf::from(std::ffi::OsString::from_wide(&output[..length as usize]))
}

#[test]
fn windows_host_short_paths_do_not_relax_agent_name_denials() {
    let (_temp, root) = tests::root();
    let workspace = root.join("runner profile");
    std::fs::create_dir(&workspace).unwrap();
    let tools = workspace.join("program files");
    std::fs::create_dir(&tools).unwrap();
    native_executable(&tools.join("utility.exe"));
    std::fs::write(tools.join("note.txt"), "native fixture").unwrap();
    assert!(fs::relative(&workspace, workspace.join("backup~2").to_str().unwrap()).is_err());
    let short_workspace = short_path(&workspace);
    let short_tools = short_path(&tools);
    assert_ne!(
        short_workspace.file_name(),
        workspace.file_name(),
        "Windows peer must generate short aliases"
    );
    let context = tests::context(&short_workspace);
    install(&context, Some(tests::capabilities())).unwrap();
    let installed = bridge(&context).unwrap();
    assert_eq!(
        installed.root.canonicalize().unwrap(),
        workspace.canonicalize().unwrap()
    );
    let alias_tail = short_workspace
        .join(short_tools.file_name().unwrap())
        .join("utility.exe");
    assert!(installed
        .file_relative(alias_tail.to_str().unwrap())
        .is_err());
    let original_spelling = short_workspace.join("program files/note.txt");
    let relative = installed
        .file_relative(original_spelling.to_str().unwrap())
        .unwrap();
    assert_eq!(relative, Path::new("program files/note.txt"));
    assert_eq!(
        fs::read(
            &installed.root,
            &installed.root_handle,
            &relative,
            None,
            None
        )
        .unwrap(),
        "native fixture"
    );
    let command = terminal::Command {
        command: "unused.exe".into(),
        args: vec![],
        cwd: Some(short_workspace.to_string_lossy().into_owned()),
        env: vec![],
        output_byte_limit: None,
    };
    assert_eq!(terminal_cwd(&installed, &command).unwrap(), installed.root);
    let env = BTreeMap::from([("PATH".into(), short_tools.to_string_lossy().into_owned())]);
    let program = terminal::windows::program("utility", &env, false).unwrap();
    assert_eq!(
        program.path.canonicalize().unwrap(),
        tools.join("utility.exe").canonicalize().unwrap()
    );
    BRIDGES.lock().remove(&context.tab_id);
}

#[test]
fn windows_program_holds_parent_and_leaf_pins_until_spawn_boundary() {
    let (_temp, root) = tests::root();
    let tools = root.join("tools");
    std::fs::create_dir(&tools).unwrap();
    let file = tools.join("utility.exe");
    native_executable(&file);
    let env = BTreeMap::from([("PATH".into(), tools.to_string_lossy().into_owned())]);
    let program = terminal::windows::program("utility", &env, false).unwrap();
    assert!(std::fs::rename(&file, tools.join("replaced.exe")).is_err());
    assert!(std::fs::write(&file, "mutated").is_err());
    assert!(std::fs::rename(&tools, root.join("moved")).is_err());
    drop(program);
    std::fs::rename(&file, tools.join("replaced.exe")).unwrap();
}

#[test]
fn windows_agent_path_cannot_use_host_aliases_or_reparse_entries() {
    let (_temp, root) = tests::root();
    let tools = root.join("program files");
    std::fs::create_dir(&tools).unwrap();
    native_executable(&tools.join("utility.exe"));
    let short_tools = short_path(&tools);
    assert_ne!(
        short_tools.file_name(),
        tools.file_name(),
        "Windows peer must generate short aliases"
    );
    let inherited = BTreeMap::from([("PATH".into(), short_tools.to_string_lossy().into_owned())]);
    assert!(terminal::windows::program("utility", &inherited, true).is_err());
    assert!(terminal::environment(
        HashMap::new(),
        &[terminal::Env {
            name: "pAtH".into(),
            value: short_tools.to_string_lossy().into_owned()
        }],
        &[]
    )
    .is_err());
    let linked = root.join("linked tools");
    junction(&linked, &tools);
    let changes = [terminal::Env {
        name: "PATH".into(),
        value: linked.to_string_lossy().into_owned(),
    }];
    assert!(terminal::environment(HashMap::new(), &changes, &[]).is_err());
    for refused in [linked.join("not-yet-created"), root.join("missing~1")] {
        let changes = [terminal::Env {
            name: "PATH".into(),
            value: refused.to_string_lossy().into_owned(),
        }];
        assert!(terminal::environment(HashMap::new(), &changes, &[]).is_err());
    }
    let missing = root.join("node_modules/.bin");
    let changes = [terminal::Env {
        name: "PATH".into(),
        value: format!("{};{}", missing.display(), tools.display()),
    }];
    let with_missing = terminal::environment(HashMap::new(), &changes, &[]).unwrap();
    let resolved = terminal::windows::program("utility", &with_missing, true).unwrap();
    assert_eq!(
        resolved.path.canonicalize().unwrap(),
        tools.join("utility.exe").canonicalize().unwrap()
    );
    assert!(!missing.exists());
    assert!(terminal::windows::program("utility", &inherited, false).is_ok());
    let changes = [terminal::Env {
        name: "pAtH".into(),
        value: tools.to_string_lossy().into_owned(),
    }];
    let agent_env = terminal::environment(HashMap::new(), &changes, &[]).unwrap();
    assert_eq!(
        agent_env
            .keys()
            .filter(|key| key.eq_ignore_ascii_case("PATH"))
            .count(),
        1
    );
    assert!(terminal::windows::program("utility", &agent_env, true).is_ok());
}

fn native_executable(destination: &Path) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    use std::os::windows::ffi::OsStringExt;
    let mut directory = vec![0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(directory.as_mut_ptr(), directory.len() as u32) };
    assert!(
        length > 0 && (length as usize) < directory.len(),
        "GetSystemDirectoryW failed"
    );
    let system = PathBuf::from(std::ffi::OsString::from_wide(&directory[..length as usize]));
    std::fs::copy(system.join("cmd.exe"), destination).unwrap();
}

fn junction(link: &Path, target: &Path) {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::{
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
        System::IO::DeviceIoControl,
    };
    let target = fs::windows::host_disk_path(target).unwrap();
    let substitute: Vec<_> = std::ffi::OsString::from(format!("\\??\\{}", target.display()))
        .encode_wide()
        .collect();
    let print: Vec<_> = target.as_os_str().encode_wide().collect();
    let names_length = (substitute.len() + print.len() + 2) * 2;
    let mut data = Vec::with_capacity(16 + names_length);
    data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
    data.extend_from_slice(&u16::try_from(8 + names_length).unwrap().to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&u16::try_from(substitute.len() * 2).unwrap().to_le_bytes());
    data.extend_from_slice(
        &u16::try_from((substitute.len() + 1) * 2)
            .unwrap()
            .to_le_bytes(),
    );
    data.extend_from_slice(&u16::try_from(print.len() * 2).unwrap().to_le_bytes());
    for unit in substitute
        .into_iter()
        .chain(Some(0))
        .chain(print)
        .chain(Some(0))
    {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::create_dir(link).unwrap();
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(link)
        .unwrap();
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            0x0009_00a4,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(
        result,
        0,
        "Set junction {}: {}",
        link.display(),
        std::io::Error::last_os_error()
    );
}

fn guid_file_reparse(path: &Path) -> std::fs::File {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::{
        Foundation::ERROR_PRIVILEGE_NOT_HELD,
        Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        },
    };
    std::fs::write(path, "ordinary file").unwrap();
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    if let Err(error) = guid_reparse_control(&handle, 0x0009_00a4, b"regular") {
        if error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD as i32) {
            panic!("NOT RUN: GUID file-leaf reparse proof requires unavailable privilege (ERROR_PRIVILEGE_NOT_HELD): {error}");
        }
        panic!(
            "GUID file-leaf reparse fixture {} failed: {error}",
            path.display()
        );
    }
    let attributes = handle.metadata().unwrap().file_attributes();
    assert_eq!(
        attributes & FILE_ATTRIBUTE_DIRECTORY,
        0,
        "fixture must remain a FILE leaf"
    );
    assert_ne!(
        attributes & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "fixture must carry a reparse attribute"
    );
    handle
}

fn clear_guid_reparse(file: &std::fs::File) {
    guid_reparse_control(file, 0x0009_00ac, &[]).unwrap();
}

fn guid_reparse_control(file: &std::fs::File, control: u32, payload: &[u8]) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    const TAG: u32 = 0x0000_0042;
    const OWNER: [u8; 16] = [
        0x4e, 0x6d, 0xfd, 0xc7, 0x6a, 0x46, 0x21, 0x4f, 0x8c, 0x08, 0x60, 0x9b, 0x23, 0x46, 0xd9,
        0x70,
    ];
    let mut data = Vec::new();
    data.extend_from_slice(&TAG.to_le_bytes());
    data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&OWNER);
    data.extend_from_slice(payload);
    let mut returned = 0;
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            control,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[test]
fn windows_exclusive_reader_reports_neutral_file_locked_refusal() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_temp, root) = tests::root();
    let path = root.join("locked");
    std::fs::write(&path, "original").unwrap();
    let handle = fs::windows::open_root(&root).unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let message = fs::read(&root, &handle, Path::new("locked"), None, None).unwrap_err();
    assert!(message.to_string().starts_with("file_locked:"), "{message}");
    assert!(
        !message.to_string().contains("delete access"),
        "read locking must not claim a delete-sharing cause"
    );
    assert_eq!(
        bridge_failure(message).data,
        Some(json!({"reason":"file_locked"}))
    );
    drop(held);
    assert_eq!(
        fs::read(&root, &handle, Path::new("locked"), None, None).unwrap(),
        "original"
    );
}

#[test]
fn windows_bridge_questions_use_display_paths_without_changing_io_paths() {
    let (_temp, root) = tests::root();
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    {
        let mut context = session.context.lock();
        context.cwd = root.to_string_lossy().into_owned();
        context.active_turn_id = Some("display-turn".into());
    }
    let context = session.context.lock().clone();
    install(&context, Some(tests::capabilities())).unwrap();
    let bridge = bridge(&context).unwrap();
    std::fs::write(root.join(".env.local"), "opaque").unwrap();
    for request in [
        Request::Read {
            session_id: "own-thread".into(),
            path: root.join(".env.local").to_string_lossy().into_owned(),
            line: None,
            limit: None,
        },
        Request::Write {
            session_id: "own-thread".into(),
            path: root.join("created.txt").to_string_lossy().into_owned(),
            content: "opaque".into(),
        },
    ] {
        let question = prepare(&fixture.service, &session, &context, &bridge, &request)
            .unwrap()
            .0
            .unwrap()
            .question;
        assert!(!question.contains(r"\\?\"));
        let path = match request {
            Request::Read { path, .. } | Request::Write { path, .. } => path,
            _ => unreachable!(),
        };
        assert!(question.contains(&codeterm_paths::strip_verbatim_prefix(&path)));
    }
    assert_eq!(bridge.root, root);
    BRIDGES.lock().remove(&context.tab_id);
}
