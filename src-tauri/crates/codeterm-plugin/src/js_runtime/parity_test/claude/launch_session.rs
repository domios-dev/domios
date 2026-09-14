//! Claude launch-session parity: native process-registry detection and the
//! bundled JS plugin agree on one registry fixture.

use super::super::*;
use super::*;
use crate::agent_claude::provider::launch_session::PROCESS_REGISTRY_DIR_ENV;
use crate::provider::{
    AgentProvider, LaunchProcess, LaunchSessionDetection, LaunchSessionEvidence,
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
    let _env = EnvGuard::new(&[(PROCESS_REGISTRY_DIR_ENV, registry.path().to_str().unwrap())]);
    let (native, js) = pair();

    let own = Some(LaunchSessionDetection {
        session_id: "sid-own".into(),
        source: LaunchSessionSource::PidRegistry,
    });
    let cases = vec![
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
