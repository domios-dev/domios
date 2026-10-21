use super::*;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard, OnceLock as StdOnceLock};

fn child(pid: u32, wrapper: bool) -> KeepAwakeChild {
    KeepAwakeChild {
        pid,
        spawned_at_ms: 123,
        display: false,
        wrapper,
    }
}

fn active_test_state() -> KeepAwakeState {
    let (stop_tx, _stop_rx) = std::sync::mpsc::channel();
    Arc::new(parking_lot::Mutex::new(Some(ActiveSession {
        stop_tx,
        display: false,
        expires_at: None,
        session_id: 42,
        gnome_policy: false,
        system_block: false,
    })))
}

fn reset_child_registry(entries: Vec<KeepAwakeChild>) {
    *child_registry().lock() = entries;
}

fn registry_test_lock() -> StdMutexGuard<'static, ()> {
    static LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();
    LOCK.get_or_init(|| StdMutex::new(())).lock().unwrap()
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
fn status_reports_inactive_when_registered_child_is_dead() {
    let _guard = registry_test_lock();
    let state = active_test_state();
    reset_child_registry(vec![child(30, true)]);

    let status = status_with_inspector(&state, |_| None);

    assert!(!status.active);
    assert!(status
        .warnings
        .iter()
        .any(|warning| warning.contains("not alive")));
}

#[test]
fn supervisor_tick_respawns_once_for_active_session_with_dead_child() {
    let _guard = registry_test_lock();
    let state = active_test_state();
    reset_child_registry(vec![child(40, true)]);
    let respawns = Arc::new(StdMutex::new(0));
    let respawns_for_closure = Arc::clone(&respawns);

    let outcome = supervisor_tick_with(
        &state,
        |_| None,
        || {
            *respawns_for_closure.lock().unwrap() += 1;
            reset_child_registry(vec![child(41, true)]);
            Ok(())
        },
    );

    assert_eq!(outcome, SupervisorTick::Repaired);
    assert_eq!(*respawns.lock().unwrap(), 1);
    assert_eq!(*child_registry().lock(), vec![child(41, true)]);
}

#[test]
fn exit_cleanup_clears_active_session_state_with_empty_child_registry() {
    let _guard = registry_test_lock();
    let state = active_test_state();
    reset_child_registry(vec![child(50, true)]);

    let killed = cleanup_active_children_with_state(Some(&state), |_| Ok(()));

    assert_eq!(killed, vec![50]);
    assert!(state.lock().is_none());
    assert!(child_registry().lock().is_empty());
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

#[derive(Clone, Default)]
struct RecordingEmitter(Arc<Mutex<Vec<serde_json::Value>>>);

impl StatusEmitter for RecordingEmitter {
    fn emit_status(&self, status: &KeepAwakeStatus) {
        self.0.lock().push(serde_json::to_value(status).unwrap());
    }
}

#[test]
fn expiry_persists_and_emits_the_same_inactive_status() {
    let state = active_test_state();
    let emitter = RecordingEmitter::default();
    let mut persisted = None;
    clear_if_owner_with(
        &state,
        42,
        &emitter,
        |status| {
            assert!(state.lock().is_none());
            assert!(emitter.0.lock().is_empty());
            persisted = Some(serde_json::to_value(status).unwrap());
        },
        || None,
    );
    let emitted = emitter.0.lock();
    assert_eq!(emitted.len(), 1);
    assert_eq!(Some(&emitted[0]), persisted.as_ref());
    assert_eq!(emitted[0]["active"], false);
    assert_eq!(emitted[0]["display"], false);
    assert!(emitted[0]["expires_at"].is_null());
}

#[test]
fn stale_expiry_preserves_the_current_owner_without_emitting() {
    let state = active_test_state();
    let emitter = RecordingEmitter::default();
    clear_if_owner_with(
        &state,
        41,
        &emitter,
        |_| panic!("stale owner persisted"),
        || panic!("stale owner inspected diagnostics"),
    );
    assert_eq!(state.lock().as_ref().unwrap().session_id, 42);
    assert!(emitter.0.lock().is_empty());
}
