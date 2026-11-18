use super::*;

#[test]
fn selects_folder_and_full_concierge_shells() {
    let mut policy = ExecutionSandboxPolicy {
        intent: ExecutionIntent::Mutating,
        cwd: "C:/friend".into(),
        launch_args: vec![],
        runtime_mode: Some("workspace-write".into()),
        writable_roots: vec!["C:/own-pad".into()],
        environment: Default::default(),
    };
    assert!(selects_native_shell(true, Some(&policy), false));
    assert!(selects_native_shell(true, None, true));
    assert!(!selects_native_shell(false, Some(&policy), false));
    assert!(!selects_native_shell(false, None, true));
    assert!(!selects_native_shell(true, None, false));
    policy.intent = ExecutionIntent::ReadOnly;
    policy.runtime_mode = Some("read-only-with-writable-roots".into());
    assert!(!selects_native_shell(true, Some(&policy), false));
}

#[test]
fn resolves_store_aliases_to_native_powershell() {
    let alias = PathBuf::from(r"C:\Users\fixture\AppData\Local\Microsoft\WindowsApps");
    let packaged = alias.join("Microsoft.PowerShell_8wekyb3d8bbwe");
    let linked = PathBuf::from(r"C:\LinkedShell");
    let tools = PathBuf::from(r"C:\Tools");
    let core = PathBuf::from(r"C:\Program Files\PowerShell\7");
    let windows = PathBuf::from(r"D:\Windows\System32\WindowsPowerShell\v1.0");
    let inherited = env::join_paths([&alias, &packaged, &linked, &tools]).unwrap();

    for native_dir in [&core, &windows] {
        let native_exe = native_dir.join(if native_dir == &core {
            "pwsh.exe"
        } else {
            "powershell.exe"
        });
        let resolved = resolve_path(&inherited, [core.clone()], Some(windows.clone()), |path| {
            if path == linked.join("pwsh.exe") {
                Some(packaged.join("pwsh.exe"))
            } else {
                (path == native_exe).then(|| native_exe.clone())
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            env::split_paths(&resolved).collect::<Vec<_>>(),
            [native_dir.clone(), tools.clone()]
        );
    }

    let native_path = env::join_paths([&core, &tools]).unwrap();
    assert_eq!(
        resolve_path(&native_path, [], None, |_| None).unwrap(),
        None
    );
    assert!(resolve_path(&inherited, [], None, |_| None).is_err());
}
