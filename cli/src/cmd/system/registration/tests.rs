use super::*;
use crate::cmd::system::{build_linux_unit, build_macos_plist, build_windows_task_command};

#[test]
fn launchagent_target_round_trips_escaped_installed_path() {
    let plist = build_macos_plist(
        "com.domios.app",
        "/Users/O'Neil & Co/cli/domios",
        "/Applications/Domios.app/Contents/MacOS/domios-app",
    );
    assert_eq!(
        macos_target(&plist),
        Some("/Users/O'Neil & Co/cli/domios".into())
    );
    assert_eq!(
        macos_target("<plist><key>Label</key><string>domios</string></plist>"),
        None
    );
}

#[test]
fn task_target_round_trips_powershell_quoting_and_xml_entities() {
    let command = build_windows_task_command(
        r"C:\Users\O'Neil & Co\cli\domios.exe",
        r"C:\Apps\Domios\domios-app.exe",
    );
    let arguments = command.strip_prefix("powershell.exe ").unwrap();
    let task = format!("<Task><Actions><Exec><Command>powershell.exe</Command><Arguments>{}</Arguments></Exec></Actions></Task>", crate::cmd::system::xml_escape(arguments));
    assert_eq!(
        windows_target(&task),
        Some(r"C:\Users\O'Neil & Co\cli\domios.exe".into())
    );
}

#[test]
fn task_direct_supervisor_action_is_recognized() {
    let task = "<Task><Actions><Exec><Command>C:\\Apps\\Domios\\cli\\domios.exe</Command><Arguments>system supervise --binary app.exe</Arguments></Exec></Actions></Task>";
    assert_eq!(
        windows_target(task),
        Some(r"C:\Apps\Domios\cli\domios.exe".into())
    );
    assert_eq!(
        windows_target("<Task><Exec><Command>domios-app.exe</Command></Exec></Task>"),
        None
    );
}

#[test]
fn task_output_decodes_utf16_with_and_without_bom() {
    let raw = "<Task><Command>C:\\Apps\\Domios\\cli\\domios.exe</Command></Task>";
    let bytes: Vec<_> = raw.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(windows_task_text(&bytes), raw);
    let mut with_bom = vec![0xff, 0xfe];
    with_bom.extend(&bytes);
    assert_eq!(windows_task_text(&with_bom), raw);
    assert_eq!(windows_task_text(raw.as_bytes()), raw);
}

#[test]
fn systemd_target_round_trips_quoted_path() {
    let unit = build_linux_unit("/opt/Domios & Co/cli/domios", "/opt/Domios & Co/domios-app");
    assert_eq!(
        linux_target(&unit),
        Some("/opt/Domios & Co/cli/domios".into())
    );
    assert_eq!(
        linux_target(
            "[Service]\nExecStart=/usr/bin/domios system supervise --binary /usr/bin/domios-app\n"
        ),
        Some("/usr/bin/domios".into())
    );
    assert_eq!(linux_target("ExecStart=/usr/bin/domios-app\n"), None);
}

#[test]
fn systemd_target_round_trips_backslashes_and_quotes() {
    let unit = build_linux_unit("/opt/a\\b\"c/domios", "/usr/bin/domios-app");
    assert_eq!(linux_target(&unit), Some("/opt/a\\b\"c/domios".into()));
}

#[test]
fn every_registration_with_a_missing_target_reports_broken() {
    use crate::cmd::system::installed::{
        AutostartState, BrokenReason, InstalledSupervisor, REPAIR_COMMAND,
    };
    let targets = [
        macos_target(&build_macos_plist("com.domios.app", "/gone/domios", "/Applications/Domios.app/Contents/MacOS/domios-app")),
        linux_target(&build_linux_unit("/gone/domios", "/usr/bin/domios-app")),
        windows_target("<Exec><Command>C:\\gone\\domios.exe</Command><Arguments>system supervise --binary app.exe</Arguments></Exec>"),
    ];
    for target in targets {
        assert!(target.is_some());
        assert_eq!(
            AutostartState::assess(
                true,
                true,
                target.as_deref(),
                &InstalledSupervisor::new("linux", None, &[], vec![]),
                |_| None
            ),
            AutostartState::Broken {
                reason: BrokenReason::MissingTarget,
                repair_command: REPAIR_COMMAND
            }
        );
    }
}
