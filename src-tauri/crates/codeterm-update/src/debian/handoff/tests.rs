use super::*;

#[test]
fn graphical_elevation_stays_inside_the_login_session() {
    assert!(!use_systemd(Elevation::Pkexec, true));
    assert!(!use_systemd(Elevation::Pkexec, false));
    assert!(use_systemd(Elevation::SudoNonInteractive, true));
    assert!(!use_systemd(Elevation::SudoNonInteractive, false));
}

#[test]
fn service_context_requires_sudo_and_a_separate_transient_unit() {
    assert_eq!(
        launch_mode(true, true, true, true, true),
        Ok((Elevation::SudoNonInteractive, true))
    );
    assert!(launch_mode(true, true, true, false, true)
        .unwrap_err()
        .contains("session is still running"));
    assert!(launch_mode(true, true, true, true, false)
        .unwrap_err()
        .contains("session is still running"));
    assert_eq!(
        launch_mode(false, true, true, false, true),
        Ok((Elevation::Pkexec, false))
    );
    assert!(inside_systemd_unit(true, ""));
    assert!(inside_systemd_unit(
        false,
        "0::/user.slice/domios.service\n"
    ));
    assert!(!inside_systemd_unit(
        false,
        concat!("0::/user.slice/user@", "1000.service/app.slice/desktop.scope\n")
    ));
}

#[test]
fn snapshot_recovery_requires_exit_confirmation_for_every_started_pair() {
    assert!(may_restore_snapshot(true, true));
    assert!(!may_restore_snapshot(false, true));
    assert!(!may_restore_snapshot(true, false));
    assert!(!may_restore_snapshot(false, false));
}

#[test]
fn persistent_handoff_target_never_contains_the_local_credential() {
    use codeterm_types::server_bootstrap::ProcessProvenance;
    let process = ProcessProvenance {
        pid: 40,
        created: "100".into(),
        executable: "/usr/bin/domios-app".into(),
    };
    let ready = serde_json::from_value(serde_json::json!({
        "protocol": 1, "api_version": 1, "build": "fixture", "instance_id": "fixture",
        "identity": {"data": "/tmp/fixture-data", "config": "/tmp/fixture-config"},
        "epoch": "fixture", "process": process, "api_address": "127.0.0.1:1234",
        "socket_address": "fixture", "bootstrap_address": "fixture", "resources": "/fixture"
    }))
    .unwrap();
    let target = QuitTarget {
        ready,
        client: process,
        instance_locks: vec!["/fixture/instance.lock".into()],
        token: "opaque-credential".into(),
    };
    let serialized = serde_json::to_string(&target).unwrap();
    assert!(!serialized.contains("opaque-credential"));
    assert!(!serialized.contains("token"));
    let restored: QuitTarget = serde_json::from_str(&serialized).unwrap();
    assert!(restored.token.is_empty());
}

#[test]
fn failures_always_select_old_snapshot_even_when_a_service_exists() {
    assert_eq!(relaunch_target(true, true), Relaunch::OldSnapshot);
    assert_eq!(relaunch_target(true, false), Relaunch::OldSnapshot);
    assert_eq!(relaunch_target(false, true), Relaunch::Service);
    assert_eq!(relaunch_target(false, false), Relaunch::Installed);
}

#[test]
fn exact_pair_requires_both_pids_start_identities_and_executables() {
    use codeterm_types::server_bootstrap::ProcessProvenance;
    let server = ProcessProvenance {
        pid: 40,
        created: "100".into(),
        executable: "/usr/lib/domios/server".into(),
    };
    let client = ProcessProvenance {
        pid: 41,
        created: "101".into(),
        executable: "/usr/bin/domios-app".into(),
    };
    assert!(exact_pair((&server, &client), (&server, &client)));
    let mut reused = server.clone();
    reused.created = "102".into();
    assert!(!exact_pair((&server, &client), (&reused, &client)));
    let mut foreign = client.clone();
    foreign.pid += 1;
    assert!(!exact_pair((&server, &client), (&server, &foreign)));
    foreign = client.clone();
    foreign.executable = "/other/app".into();
    assert!(!exact_pair((&server, &client), (&server, &foreign)));
}

#[test]
fn elevation_uses_gui_only_in_graphical_sessions_and_never_interactive_sudo() {
    assert_eq!(elevation(true, true, true), Ok(Elevation::Pkexec));
    assert_eq!(
        elevation(false, true, true),
        Ok(Elevation::SudoNonInteractive)
    );
    assert_eq!(
        elevation(true, false, true),
        Ok(Elevation::SudoNonInteractive)
    );
    assert!(elevation(false, true, false).is_err());
}

struct Spy {
    steps: Vec<Step>,
    fail: Option<Step>,
    failures: Vec<String>,
}
#[async_trait::async_trait]
impl Port for Spy {
    async fn execute(&mut self, step: Step) -> Result<(), String> {
        self.steps.push(step);
        if self.fail == Some(step) {
            Err("injected".into())
        } else {
            Ok(())
        }
    }
    async fn recover(&mut self, failure: &str) {
        self.failures.push(failure.into());
    }
}

#[tokio::test]
async fn helper_verifies_and_pins_before_quit_and_waits_before_install_and_relaunch() {
    let mut port = Spy {
        steps: vec![],
        fail: None,
        failures: vec![],
    };
    run(&mut port).await.unwrap();
    assert_eq!(port.steps, STEPS);
    assert!(port.failures.is_empty());
}

#[tokio::test]
async fn every_failure_stops_installation_and_calls_rollback_relaunch_port() {
    for (index, step) in STEPS.into_iter().enumerate() {
        let mut port = Spy {
            steps: vec![],
            fail: Some(step),
            failures: vec![],
        };
        assert!(run(&mut port).await.is_err());
        assert_eq!(port.steps, STEPS[..=index]);
        assert_eq!(port.failures.len(), 1);
        assert!(port.failures[0].contains("injected"));
    }
}
