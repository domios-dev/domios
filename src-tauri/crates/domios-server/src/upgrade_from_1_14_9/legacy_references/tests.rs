use super::*;

fn inspect_text(text: &str, kind: ReferenceKind, missing: bool) -> Option<LegacyReferenceNotice> {
    inspect(
        Path::new("/home/owner/config"),
        text,
        kind,
        Path::new("/home/owner/.codeterm"),
        Path::new("/home/owner/.domios"),
        false,
        missing,
    )
}

#[test]
fn legacy_references_detect_path_boundaries_and_platform_spelling() {
    assert!(contains_root(
        r#"{"cwd":"/home/owner/.codeterm/general"}"#,
        "/home/owner/.codeterm",
        false
    ));
    assert!(contains_root(
        r#"{"path":"C:\\Users\\Owner\\.codeterm\\general"}"#,
        "C:/Users/Owner/.codeterm",
        true
    ));
    assert!(contains_root(
        r#"{"path":"C:\\Users\\Owner\\.codeterm\\general"}"#,
        r"\\?\C:\Users\Owner\.codeterm",
        true
    ));
    assert!(contains_root("'~/.codeterm/general'", "~/.codeterm", false));
    for text in [
        "/home/owner/.codeterm-other/general",
        "/home/owner/.codeterm-dev/general",
        "/other/home/owner/.codeterm/general",
        "/home/owner/.domios/general",
    ] {
        assert!(
            !contains_root(text, "/home/owner/.codeterm", false),
            "{text}"
        );
    }
    assert!(!contains_root(
        "/HOME/owner/.codeterm/general",
        "/home/owner/.codeterm",
        false
    ));
}

#[test]
fn legacy_references_are_typed_and_do_not_expose_config_contents() {
    let notice = inspect_text(
        r#"{"token":"opaque-secret","cwd":"~/.codeterm/general"}"#,
        ReferenceKind::StoredPath,
        false,
    )
    .unwrap();
    let json = serde_json::to_value(&notice).unwrap();
    assert_eq!(json["kind"], "stored_path");
    assert_eq!(json["file"], "/home/owner/config");
    assert!(notice.fix.contains("/home/owner/.domios"));
    assert!(!serde_json::to_string(&notice)
        .unwrap()
        .contains("opaque-secret"));
}

#[test]
fn legacy_references_report_scripts_and_plugin_code() {
    for (text, kind) in [
        (
            "model = '~/.codeterm/transcriber/model'",
            ReferenceKind::Script,
        ),
        (
            "host.fs.expandHome('~/.codeterm/telegram-client')",
            ReferenceKind::Plugin,
        ),
    ] {
        let notice = inspect_text(text, kind, false).unwrap();
        assert_eq!(notice.kind, kind);
        assert!(notice.fix.contains(if kind == ReferenceKind::Plugin {
            "host.dataDir()"
        } else {
            "codeterm pad path"
        }));
    }
}

#[test]
fn legacy_references_systemd_missing_binary_has_exact_stop_command() {
    let text = "[Service]\nExecStart=/usr/bin/codeterm --minimized\nRestart=always\n";
    assert!(missing_service_executable(
        text,
        ReferenceKind::SystemdUnit,
        |_| false
    ));
    let notice = inspect(
        Path::new("/home/owner/.config/systemd/user/codeterm.service"),
        text,
        ReferenceKind::SystemdUnit,
        Path::new("/home/owner/.codeterm"),
        Path::new("/home/owner/.domios"),
        false,
        true,
    )
    .unwrap();
    assert!(notice
        .fix
        .contains("systemctl --user disable --now 'codeterm.service'"));
    assert!(notice.fix.contains("daemon-reload"));
    assert!(notice.fix.contains("package autostart"));
    assert!(notice.fix.contains("ownership is unproven"));
    assert!(inspect_text(text, ReferenceKind::SystemdUnit, false).is_none());
}

#[test]
fn legacy_references_launchagent_reports_stale_logs_and_unproven_ownership() {
    let text = "<key>Label</key><string>com.codeterm.whisper-shim</string><key>ProgramArguments</key><array><string>/usr/bin/python3</string></array><key>StandardOutPath</key><string>/home/owner/.codeterm/logs/shim.log</string>";
    let notice = inspect_text(text, ReferenceKind::LaunchAgent, false).unwrap();
    assert_eq!(notice.kind, ReferenceKind::LaunchAgent);
    assert!(notice
        .fix
        .contains("launchctl bootout gui/$(id -u) '/home/owner/config'"));
    assert!(notice.fix.contains("update any referenced script"));
    assert!(notice.fix.contains("no file was changed"));
}

#[test]
fn legacy_references_service_probe_accepts_quoted_paths_and_current_legacy_labels() {
    let unit = "ExecStart=\"/opt/Domios App/domios\" --minimized\n";
    assert!(!missing_service_executable(
        unit,
        ReferenceKind::SystemdUnit,
        |path| path == "/opt/Domios App/domios"
    ));
    let plist = "<key>Label</key><string>com.codeterm.app</string><key>ProgramArguments</key><array><string>/Applications/Domios.app/cli/codeterm</string></array>";
    assert!(!missing_service_executable(
        plist,
        ReferenceKind::LaunchAgent,
        |_| true
    ));
    assert!(inspect_text(plist, ReferenceKind::LaunchAgent, false).is_none());
    assert!(inspect_text(
        "ExecStart=/usr/bin/unrelated\n",
        ReferenceKind::SystemdUnit,
        true
    )
    .is_none());
}

#[test]
fn legacy_references_scan_known_files_without_rewriting_trusted_plugins() {
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join(".domios-dev");
    let plugin = data.join("plugins/fixture");
    fs::create_dir_all(&plugin).unwrap();
    let source = "host.fs.expandHome('~/.codeterm-dev/fixture')";
    fs::write(plugin.join("plugin.js"), source).unwrap();
    fs::write(
        plugin.join("plugin.json"),
        r#"{"dataPaths":["~/.codeterm-dev/fixture"]}"#,
    )
    .unwrap();
    fs::write(data.join("config.json"), "{}").unwrap();
    let result = scan(&data, &data.join("config.json"));
    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .all(|notice| notice.kind == ReferenceKind::Plugin));
    assert_eq!(
        fs::read_to_string(plugin.join("plugin.js")).unwrap(),
        source
    );
    assert!(!home.path().join(".codeterm-dev").exists());
}

#[test]
fn legacy_references_report_once_persists_and_changes_reopen_notices() {
    let data = tempfile::tempdir().unwrap();
    let first = inspect_text("~/.codeterm/general", ReferenceKind::StoredPath, false).unwrap();
    let second = inspect_text("~/.codeterm/general2", ReferenceKind::StoredPath, false).unwrap();
    let mut reported = Vec::new();
    report_once(data.path(), vec![first.clone(), first.clone()], |notice| {
        reported.push(notice.clone())
    });
    assert_eq!(reported.len(), 1);
    report_once(data.path(), vec![first.clone()], |_| {
        panic!("unchanged notice repeated")
    });
    report_once(data.path(), vec![second], |notice| {
        reported.push(notice.clone())
    });
    assert_eq!(reported.len(), 2);
    report_once(data.path(), Vec::new(), |_| panic!("no references"));
    report_once(data.path(), vec![first], |notice| {
        reported.push(notice.clone())
    });
    assert_eq!(reported.len(), 3);
}

#[test]
fn legacy_references_inventory_records_legacy_pad_file_and_mtime() {
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join(".domios");
    fs::create_dir_all(data.join("storage")).unwrap();
    fs::write(data.join("storage/state.db"), []).unwrap();
    let pad = home.path().join(".codeterm/runtime/agent-pads/894da314");
    fs::create_dir_all(&pad).unwrap();
    let file = pad.join("i1-ready.md");
    fs::write(&file, "opaque payload").unwrap();
    let result = scan_legacy_writes(&data);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].kind, ReferenceKind::UnexpectedWrite);
    assert_eq!(result[0].file, file);
    assert!(result[0].modified_ms.is_some());
    assert_eq!(fs::read_to_string(&file).unwrap(), "opaque payload");
}

#[test]
fn legacy_references_errors_are_visible_without_blocking_startup() {
    let data = tempfile::tempdir().unwrap();
    fs::create_dir_all(data.path().join("runtime")).unwrap();
    fs::write(
        data.path().join("runtime/legacy-reference-notices.json"),
        "broken JSON",
    )
    .unwrap();
    let mut reported = Vec::new();
    report_once(data.path(), Vec::new(), |notice| {
        reported.push(notice.clone())
    });
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].kind, ReferenceKind::ScanFailure);
}
