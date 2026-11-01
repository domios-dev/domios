use super::*;
use codeterm_util::autostart::{build_linux_unit, build_macos_plist, build_windows_task_command};

#[test]
fn installed_application_candidates_never_select_a_cli_command() {
    let home = Some(std::path::PathBuf::from("/home/owner"));
    for platform in ["windows", "macos", "linux"] {
        let paths = installed_app_candidates(
            platform,
            Some(std::path::PathBuf::from("/LocalAppData")),
            Some(std::path::PathBuf::from("/ProgramFiles")),
            home.clone(),
        );
        assert!(!paths.is_empty());
        for path in paths {
            assert_eq!(
                path.file_name().unwrap(),
                if platform == "windows" {
                    "domios-app.exe"
                } else {
                    "domios-app"
                }
            );
            assert!(!path.to_string_lossy().contains("CodeTerm"));
        }
    }
    assert_eq!(
        installed_app_candidates("linux", None, None, home),
        vec![
            std::path::PathBuf::from("/usr/local/bin/domios-app"),
            std::path::PathBuf::from("/usr/bin/domios-app"),
            std::path::PathBuf::from("/home/owner/.local/bin/domios-app"),
        ]
    );
}

#[test]
fn macos_plist_contains_label_and_binary_and_keepalive_and_runatload() {
    let plist = build_macos_plist(
        "com.domios.app",
        "/Users/me/.domios/bin/codeterm",
        "/Applications/Domios.app/Contents/MacOS/domios-app",
    );
    assert!(plist.contains("<string>com.domios.app</string>"));
    assert!(plist.contains("<string>/Users/me/.domios/bin/codeterm</string>"));
    assert!(plist.contains("<string>system</string>"));
    assert!(plist.contains("<string>supervise</string>"));
    assert!(plist.contains("/Applications/Domios.app/Contents/MacOS/domios-app"));
    assert!(plist.contains("<key>KeepAlive</key>\n    <dict>\n        <key>SuccessfulExit</key>\n        <false/>\n    </dict>"));
    assert!(plist.contains("<key>ThrottleInterval</key>\n    <integer>30</integer>"));
    assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
    assert!(plist.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
}

#[test]
fn linux_unit_runs_supervisor_and_user_target() {
    let unit = build_linux_unit("/usr/bin/domios", "/usr/bin/domios-app");
    assert!(unit.contains(
        "ExecStart=\"/usr/bin/domios\" system supervise --binary \"/usr/bin/domios-app\""
    ));
    assert!(unit.contains("Restart=on-failure\nRestartSec=30"));
    assert!(unit.contains("StartLimitIntervalSec=300\nStartLimitBurst=5"));
    assert!(unit.contains("WantedBy=default.target"));
    assert!(unit.contains("[Service]"));
    assert!(unit.contains("[Install]"));
}

#[test]
fn linux_unit_handles_path_with_spaces() {
    let unit = build_linux_unit("/home/me/Code Term/codeterm", "/opt/Code Term/codeterm");
    assert!(unit.contains(
        "ExecStart=\"/home/me/Code Term/codeterm\" system supervise --binary \"/opt/Code Term/codeterm\""
    ));
}

#[test]
fn macos_plist_escapes_xml_metacharacters() {
    let plist = build_macos_plist(
        "com.domios.app",
        "/Users/me/Code & Term/codeterm",
        "/Applications/Domios <App>.app/Contents/MacOS/domios-app",
    );
    assert!(plist.contains("/Users/me/Code &amp; Term/codeterm"));
    assert!(plist.contains("/Applications/Domios &lt;App&gt;.app"));
}

#[test]
fn windows_task_runs_supervisor_not_app_directly() {
    let command = build_windows_task_command(
        r"C:\Users\me\.domios\bin\codeterm.exe",
        r"C:\Users\me\AppData\Local\Domios\domios-app.exe",
    );
    assert!(command.contains("system"));
    assert!(command.contains("supervise"));
    assert!(command.contains("--binary"));
    assert!(command.contains(r"C:\Users\me\.domios\bin\codeterm.exe"));
    assert!(command.contains(r"C:\Users\me\AppData\Local\Domios\domios-app.exe"));
    assert!(command.contains("; & '"));
    assert!(command.contains("exit $LASTEXITCODE"));
}

#[test]
fn supervisor_lock_is_exclusive_until_the_handle_is_dropped() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("supervisor.lock");
    let first = SupervisorLock::acquire_at(&path).unwrap();
    let error = SupervisorLock::acquire_at(&path).err().unwrap();
    assert!(error.contains("another Domios supervisor is already active"));
    drop(first);
    assert!(path.is_file(), "the stable lock inode must not be unlinked");
    let second = SupervisorLock::acquire_at(&path).unwrap();
    assert!(SupervisorLock::acquire_at(&path).is_err());
    drop(second);
}

#[test]
fn supervisor_lock_ignores_a_leftover_file_naming_a_live_unrelated_process() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("recovery/supervisor.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // This test process is alive but does not own the file lock.
    std::fs::write(&path, format!("{}\n", std::process::id())).unwrap();
    let lock = SupervisorLock::acquire_at(&path).unwrap();
    drop(lock);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("{}\n", std::process::id())
    );
}

#[test]
fn windows_task_retries_failures_with_bounded_count_and_preserves_long_running_supervision() {
    let policy = codeterm_util::autostart::windows_task_xml(
        "DOMAIN\\user",
        "C:\\domios.exe",
        "C:\\domios-app.exe",
    );
    assert!(policy.contains("<Count>3</Count>"));
    assert!(policy.contains("<Interval>PT1M</Interval>"));
    assert!(policy.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
}

#[test]
fn supervisor_stops_only_for_clean_matching_identity() {
    let state = recovery_state("epoch-1", Some(("epoch-1", "user_exit")));
    assert!(clean_matches(
        state.last_clean_shutdown.as_ref(),
        &state.current
    ));
}

#[test]
fn supervisor_does_not_stop_for_stale_clean_epoch() {
    let state = recovery_state("epoch-2", Some(("epoch-1", "update")));
    assert!(!clean_matches(
        state.last_clean_shutdown.as_ref(),
        &state.current
    ));
}

#[test]
fn supervisor_classifies_pre_marker_exit_as_fast_fail() {
    let state = recovery_state("supervisor-launch", None);
    assert_eq!(classify(&state.current, 20), "fast_fail");
}

#[test]
fn supervisor_follows_only_restart_and_update_replacements() {
    let restart = recovery_state("epoch-1", Some(("epoch-1", "restart")));
    let user_exit = recovery_state("epoch-1", Some(("epoch-1", "user_exit")));
    assert!(should_follow_replacement(
        restart.last_clean_shutdown.as_ref().unwrap()
    ));
    assert!(!should_follow_replacement(
        user_exit.last_clean_shutdown.as_ref().unwrap()
    ));
}

#[test]
fn supervisor_backoff_is_bounded() {
    assert_eq!(bounded_backoff_ms(1), 1_000);
    assert_eq!(bounded_backoff_ms(3), 4_000);
    assert_eq!(bounded_backoff_ms(99), 30_000);
}

fn recovery_state(epoch: &str, clean: Option<(&str, &str)>) -> RecoveryState {
    RecoveryState {
        current: ProcessEpoch {
            epoch: epoch.into(),
            pid: 42,
            started_at: 10,
            last_healthy_at: 20,
            build_hash: "abc123".into(),
            platform: "windows".into(),
        },
        last_clean_shutdown: clean.map(|(epoch, reason)| CleanShutdownRecord {
            epoch: Some(epoch.into()),
            reason: reason.into(),
            pid: 42,
            at: 40,
        }),
    }
}
