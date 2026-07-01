use super::*;
use std::sync::{Arc, Mutex as StdMutex};

fn child(pid: u32, wrapper: bool) -> KeepAwakeChild {
    KeepAwakeChild {
        pid,
        spawned_at_ms: 123,
        display: false,
        wrapper,
    }
}

#[test]
fn child_journal_round_trips_and_empty_save_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CHILDREN_FILE);
    let entries = vec![child(10, true), child(11, false)];

    save_children_to(&path, &entries);
    assert_eq!(load_children_from(&path), entries);

    save_children_to(&path, &[]);
    assert!(!path.exists());
}

#[test]
fn reap_kills_only_orphaned_keepawake_processes() {
    let entries = vec![child(10, true), child(11, false), child(12, false)];
    let killed = Arc::new(StdMutex::new(Vec::new()));
    let killed_for_closure = Arc::clone(&killed);

    let outcome = reap_journaled_orphans(
        entries,
        |pid| match pid {
            10 => Some(ProcessSnapshot {
                ppid: 1,
                command: "/bin/sh codeterm-keepawake-wrapper".into(),
            }),
            11 => Some(ProcessSnapshot {
                ppid: 10,
                command: "caffeinate -i -s".into(),
            }),
            12 => Some(ProcessSnapshot {
                ppid: 1,
                command: "python something_else.py".into(),
            }),
            _ => None,
        },
        |pid| {
            killed_for_closure.lock().unwrap().push(pid);
            Ok(())
        },
    );

    assert_eq!(*killed.lock().unwrap(), vec![10]);
    assert_eq!(outcome.killed, vec![10]);
    assert_eq!(outcome.kept, vec![child(11, false)]);
    assert_eq!(outcome.skipped, vec![12]);
}

#[test]
fn exit_cleanup_attempts_all_registered_children() {
    let entries = vec![child(20, true), child(21, false)];
    let killed = Arc::new(StdMutex::new(Vec::new()));
    let killed_for_closure = Arc::clone(&killed);

    let result = cleanup_child_entries(entries, |pid| {
        killed_for_closure.lock().unwrap().push(pid);
        Ok(())
    });

    assert_eq!(result, vec![20, 21]);
    assert_eq!(*killed.lock().unwrap(), vec![20, 21]);
}

#[test]
fn parse_ps_snapshot_reads_ppid_and_command() {
    let snapshot = parse_ps_snapshot("    1 /bin/sh codeterm-keepawake-wrapper -i -s\n")
        .expect("snapshot parsed");

    assert_eq!(snapshot.ppid, 1);
    assert_eq!(snapshot.command, "/bin/sh codeterm-keepawake-wrapper -i -s");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_inhibit_parser_detects_codeterm_sleep_and_idle_blocks() {
    let output = "\
WHO      UID USER PID COMM            WHAT       WHY                         MODE
CodeTerm 1000 me   42 systemd-inhibit sleep:idle CodeTerm keepawake session block
";

    assert!(platform::parse_systemd_inhibit_list(output, "sleep"));
    assert!(platform::parse_systemd_inhibit_list(output, "idle"));
    assert!(!platform::parse_systemd_inhibit_list(
        "GNOME 1000 me 11 gsd-power sleep user active block",
        "sleep"
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_gnome_policy_flags_unsafe_suspend_and_idle_settings() {
    let settings = vec![
        (
            "org.gnome.settings-daemon.plugins.power".to_string(),
            "sleep-inactive-battery-type".to_string(),
            "'suspend'".to_string(),
        ),
        (
            "org.gnome.desktop.session".to_string(),
            "idle-delay".to_string(),
            "uint32 300".to_string(),
        ),
        (
            "org.gnome.settings-daemon.plugins.power".to_string(),
            "lid-close-ac-action".to_string(),
            "'nothing'".to_string(),
        ),
    ];

    let unsafe_settings = platform::unsafe_gnome_settings(&settings);

    assert_eq!(
        unsafe_settings,
        vec![
            "org.gnome.settings-daemon.plugins.power.sleep-inactive-battery-type='suspend'",
            "org.gnome.desktop.session.idle-delay=uint32 300",
        ]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_gnome_policy_accepts_nothing_and_zero_idle_delay() {
    let settings = vec![
        (
            "org.gnome.settings-daemon.plugins.power".to_string(),
            "sleep-inactive-ac-type".to_string(),
            "'nothing'".to_string(),
        ),
        (
            "org.gnome.settings-daemon.plugins.power".to_string(),
            "power-button-action".to_string(),
            "'nothing'".to_string(),
        ),
        (
            "org.gnome.desktop.session".to_string(),
            "idle-delay".to_string(),
            "uint32 0".to_string(),
        ),
    ];

    assert!(platform::unsafe_gnome_settings(&settings).is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_systemd_mask_report_lists_unmasked_units() {
    let statuses = vec![
        ("sleep.target".to_string(), "masked".to_string()),
        ("suspend.target".to_string(), "enabled".to_string()),
        ("systemd-suspend.service".to_string(), "static".to_string()),
    ];

    assert_eq!(
        platform::missing_systemd_masks(&statuses),
        vec!["suspend.target", "systemd-suspend.service"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_daemon_reload_parser_accepts_boolean_forms() {
    assert!(platform::parse_systemd_daemon_reload_needed("yes\n"));
    assert!(platform::parse_systemd_daemon_reload_needed("true\n"));
    assert!(platform::parse_systemd_daemon_reload_needed("1\n"));
    assert!(!platform::parse_systemd_daemon_reload_needed("no\n"));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_wrapper_kills_caffeinate_when_parent_disappears() {
    let script = platform::keepawake_wrapper_script();

    assert!(script.contains("caffeinate \"$@\" &"));
    assert!(script.contains("printf '%s\\n' \"$child\""));
    assert!(script.contains("trap 'cleanup; exit 0' TERM INT HUP"));
    assert!(script.contains("[ \"$ppid\" = \"1\" ]"));
    assert!(script.contains("cleanup"));
}
