//! Claude launch-session parity: native process-registry detection and the
//! bundled JS plugin agree on one registry fixture.

use super::*;
use crate::agent_claude::provider::launch_session::PROCESS_REGISTRY_DIR_ENV;
use crate::provider::{
    AgentLaunchParams, AgentProvider, LaunchProcess, LaunchSessionDetection, LaunchSessionEvidence,
    LaunchSessionSource,
};
use serde_json::json;

fn process(pid: u32, depth: u32, start_token: &str) -> LaunchProcess {
    LaunchProcess {
        pid,
        depth,
        start_token: start_token.into(),
    }
}

fn evidence(processes: Vec<LaunchProcess>) -> LaunchSessionEvidence {
    LaunchSessionEvidence {
        cwd: "/work".into(),
        launch_marker: None,
        launched_at_ms: 0,
        processes,
    }
}

#[test]
fn launch_session_detection_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let registry = tempfile::tempdir().unwrap();
    let write = |pid: u32, record: serde_json::Value| {
        std::fs::write(
            registry.path().join(format!("{pid}.json")),
            record.to_string(),
        )
        .unwrap();
    };
    write(
        200,
        json!({"pid": 200, "sessionId": "sid-own", "procStart": "5000", "kind": "interactive"}),
    );
    write(
        300,
        json!({"pid": 300, "sessionId": "sid-nested", "procStart": "6000", "kind": "interactive"}),
    );
    write(
        400,
        json!({"pid": 400, "sessionId": "sid-reused", "procStart": "1000", "kind": "interactive"}),
    );
    write(
        500,
        json!({"pid": 500, "sessionId": "sid-print", "procStart": "7000", "kind": "print"}),
    );
    let _env = EnvGuard::new(&[
        (PROCESS_REGISTRY_DIR_ENV, registry.path().to_str().unwrap()),
        ("LC_ALL", "fr_FR.UTF-8"),
        ("TZ", "America/New_York"),
    ]);
    let (native, js) = pair();

    let own = Some(LaunchSessionDetection {
        session_id: "sid-own".into(),
        source: LaunchSessionSource::PidRegistry,
    });
    let mut cases = vec![
        (
            evidence(vec![
                process(100, 0, "4000"),
                process(300, 2, "6000"),
                process(200, 1, "5000"),
            ]),
            own,
        ),
        (evidence(vec![process(400, 1, "9000")]), None),
        (evidence(vec![process(600, 1, "5000")]), None),
        (evidence(vec![process(500, 1, "7000")]), None),
        (evidence(Vec::new()), None),
    ];
    if cfg!(target_os = "macos") {
        use chrono::TimeZone;
        let fresh = AgentLaunchParams::default();
        let resume = crate::provider::AgentResumeParams {
            session_id: "own".into(),
            ..Default::default()
        };
        for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
            for params in [
                crate::provider::LaunchSpecParams::Fresh(&fresh),
                crate::provider::LaunchSpecParams::Resume(&resume),
            ] {
                let spec = provider.launch_spec(&params).unwrap();
                assert_eq!(spec.env_set.get("LC_ALL").map(String::as_str), Some("C"));
                assert_eq!(spec.env_set.get("TZ").map(String::as_str), Some("UTC0"));
            }
            for command in [
                provider.build_launch_command(&fresh),
                provider.build_resume_command_with_context(&resume).unwrap(),
            ] {
                assert!(command.contains("LC_ALL=C TZ=UTC0 "));
            }
        }
        let start = chrono::Utc
            .with_ymd_and_hms(2026, 11, 1, 5, 30, 0)
            .single()
            .unwrap()
            .timestamp_micros();
        let later = start + 3_600_000_000;
        for (pid, timestamp) in [
            (2860, "Sun Nov  1 05:30:00 2026"),
            (2861, "Sun Nov  1 06:30:00 2026"),
            (2862, "Sun Nov  1 01:30:00 2026"),
            (2863, "dim.  1 nov. 01:30:00 2026"),
        ] {
            write(
                pid,
                json!({"pid":pid,"sessionId":format!("sid-{pid}"),"procStart":timestamp,"kind":"interactive"}),
            );
        }
        cases.extend([
            (
                evidence(vec![process(2860, 1, &start.to_string())]),
                Some(LaunchSessionDetection {
                    session_id: "sid-2860".into(),
                    source: LaunchSessionSource::PidRegistry,
                }),
            ),
            (evidence(vec![process(2860, 1, &later.to_string())]), None),
            (
                evidence(vec![process(2861, 1, &later.to_string())]),
                Some(LaunchSessionDetection {
                    session_id: "sid-2861".into(),
                    source: LaunchSessionSource::PidRegistry,
                }),
            ),
        ]);
        for timestamp in [start, later] {
            for pid in [2862, 2863] {
                cases.push((
                    evidence(vec![process(pid, 1, &timestamp.to_string())]),
                    None,
                ));
            }
        }
    }
    for (index, (evidence, expected)) in cases.into_iter().enumerate() {
        assert_eq!(
            native.detect_launch_session(&evidence),
            expected,
            "native case {index}"
        );
        assert_eq!(
            js.detect_launch_session(&evidence),
            expected,
            "js case {index}"
        );
    }
}
