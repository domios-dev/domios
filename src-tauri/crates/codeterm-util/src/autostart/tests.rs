use super::*;

#[test]
fn shared_linux_unit_restarts_failures_with_a_bounded_rate() {
    let unit = build_linux_unit("/home/me/Domios CLI/domios", "/opt/Domios App/domios-app");
    assert!(unit.contains("ExecStart=\"/home/me/Domios CLI/domios\" system supervise --binary \"/opt/Domios App/domios-app\""));
    assert!(unit.contains("StartLimitIntervalSec=300\nStartLimitBurst=5"));
    assert!(unit.contains("Restart=on-failure\nRestartSec=30"));
    assert!(!unit.contains("Restart=always"));
}

#[test]
fn shared_macos_agent_retries_unsuccessful_exits_without_reopening_after_clean_exit() {
    let plist = build_macos_plist(MACOS_LABEL, "/Users/O'Neil & Co/domios", "/Applications/Domios.app");
    assert!(plist.contains("<key>SuccessfulExit</key>\n        <false/>"));
    assert!(plist.contains("<key>ThrottleInterval</key>\n    <integer>30</integer>"));
    assert!(plist.contains("O&apos;Neil &amp; Co"));
}

#[test]
fn shared_windows_task_tracks_supervisor_exit_and_retries_three_times() {
    let supervisor = format!("C:\\Users\\{}\\domios & co.exe", "a".repeat(200));
    let xml = windows_task_xml("DOMAIN\\user", &supervisor, "C:\\Apps\\domios-app.exe");
    assert!(xml.contains("<RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>"));
    assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    assert!(xml.contains("exit $LASTEXITCODE"));
    assert!(xml.contains("&amp; &apos;C:"));
    assert!(!xml.contains("Start-Process"));
    assert!(xml.contains(&supervisor.replace('&', "&amp;")));
    let bytes = utf16le_with_bom(&xml);
    assert_eq!(&bytes[..2], &[0xff, 0xfe]);
    assert_eq!(String::from_utf16(&bytes[2..].chunks_exact(2).map(|unit| u16::from_le_bytes([unit[0], unit[1]])).collect::<Vec<_>>()).unwrap(), xml);
}

#[test]
fn shared_install_plans_write_the_definition_before_os_registration_and_start() {
    let home = Path::new("/fixture/home");
    let task = Path::new("/fixture/task.xml");
    for platform in [Platform::Windows, Platform::Macos, Platform::Linux] {
        let steps = install_plan(platform, home, task, "fixture-user", "/fixture/cli", "/fixture/app");
        let Step::Write(path, bytes) = &steps[0] else { panic!("registration must follow the definition write") };
        let (expected_path, expected_bytes, expected_commands) = match platform {
            Platform::Windows => (task.to_path_buf(), utf16le_with_bom(&windows_task_xml("fixture-user", "/fixture/cli", "/fixture/app")), vec![
                run("schtasks", &["/Delete", "/TN", WINDOWS_TASK, "/F"], false),
                run("schtasks", &["/Create", "/TN", WINDOWS_TASK, "/XML", "/fixture/task.xml", "/F"], true),
                run("schtasks", &["/Run", "/TN", WINDOWS_TASK], true),
            ]),
            Platform::Macos => (macos_path(home), build_macos_plist(MACOS_LABEL, "/fixture/cli", "/fixture/app").into_bytes(), vec![
                run("launchctl", &["unload", &macos_path(home).to_string_lossy()], false),
                run("launchctl", &["load", &macos_path(home).to_string_lossy()], true),
            ]),
            Platform::Linux => (linux_path(home), build_linux_unit("/fixture/cli", "/fixture/app").into_bytes(), vec![
                run("systemctl", &["--user", "daemon-reload"], true),
                run("systemctl", &["--user", "enable", LINUX_UNIT], true),
                run("systemctl", &["--user", "start", LINUX_UNIT], true),
            ]),
        };
        assert_eq!(path, &expected_path);
        assert_eq!(bytes, &expected_bytes);
        let commands: Vec<_> = steps.into_iter().filter(|step| matches!(step, Step::Run { .. })).collect();
        assert_eq!(commands, expected_commands);
    }
}

#[test]
fn shared_file_steps_and_disable_marker_use_only_the_supplied_roots() {
    let root = tempfile::tempdir().unwrap();
    let definition = root.path().join("home/unit");
    execute(vec![Step::Write(definition.clone(), b"opaque definition".to_vec())]).unwrap();
    assert_eq!(std::fs::read(&definition).unwrap(), b"opaque definition");
    set_disabled(root.path(), true).unwrap();
    let disabled = root.path().join("state/recovery/supervisor.disabled");
    assert!(disabled.exists());
    set_disabled(root.path(), false).unwrap();
    assert!(!disabled.exists());
    execute(vec![Step::Remove(definition.clone())]).unwrap();
    assert!(!definition.exists());
}
