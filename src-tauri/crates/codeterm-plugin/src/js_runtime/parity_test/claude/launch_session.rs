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
fn exact_switch_preflight_reads_shared_transcript_without_a_pid_record() {
    use crate::provider_account::{self, with_context, InvocationContext, SharedReadGrant};
    if require_plugins().is_none() {
        return;
    }
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(
        &std::fs::read_to_string(plugin_fixture_root().join("plugin.json")).unwrap(),
    )
    .unwrap();
    let capability = manifest.accounts.unwrap();
    let home_environment = &capability.home_environment;
    let temp = tempfile::tempdir().unwrap();
    let usual = temp.path().join("usual");
    let accounts = temp.path().join("provider-accounts");
    let named = accounts.join(&manifest.id).join("named");
    let foreign = accounts.join(&manifest.id).join("foreign");
    let projects = usual.join("projects");
    let project = projects.join("-actual-project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(named.join("sessions")).unwrap();
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::create_dir_all(usual.join("sessions")).unwrap();
    let shared_link = named.join("projects");
    let undeclared_alias = named.join("undeclared");
    codeterm_util::fs_links::directory_link(&projects, &shared_link).unwrap();
    codeterm_util::fs_links::directory_link(&projects, &undeclared_alias).unwrap();
    codeterm_util::fs_links::directory_link(&projects, &foreign.join("projects")).unwrap();
    let id = "d3028899-74bc-4921-824b-5332bef83182";
    let default_record = usual.join("sessions/123.json");
    std::fs::write(
        &default_record,
        json!({"pid": 123, "sessionId": id}).to_string(),
    )
    .unwrap();
    assert!(!named.join("sessions/123.json").exists());
    let evidence = temp.path().join("evidence");
    let _env = EnvGuard::new(&[
        (home_environment.as_str(), usual.to_str().unwrap()),
        (
            crate::session_evidence::EVIDENCE_DIR_ENV,
            evidence.to_str().unwrap(),
        ),
    ]);
    let invocation = InvocationContext::from_capability(
        manifest.id,
        "named".into(),
        &capability,
        [(
            capability.home_environment.clone(),
            Some(named.to_string_lossy().into_owned()),
        )]
        .into(),
        vec![named.clone()],
        vec![usual.clone(), accounts.clone()],
    )
    .unwrap()
    .with_shared_exclusions(vec![accounts, named.clone(), foreign.clone()])
    .unwrap()
    .with_provisioned_shared_reads(
        vec![SharedReadGrant::from_layout(
            capability
                .shared_paths
                .iter()
                .find(|layout| layout.path == "projects")
                .unwrap(),
            &usual,
        )
        .unwrap()],
        [(shared_link.clone(), projects.canonicalize().unwrap())].into(),
    )
    .unwrap();
    let transcript = project.join(format!("{id}.jsonl"));
    std::fs::write(&transcript, "{}\n").unwrap();
    let (native, js) = pair();
    with_context(Some(invocation.clone()), || {
        assert!(provider_account::read_allowed(&projects));
        assert!(!provider_account::read_allowed(&shared_link));
        assert!(!provider_account::read_allowed(&undeclared_alias));
        assert!(!provider_account::read_allowed(&foreign.join("projects")));
        assert!(!provider_account::read_allowed(&default_record));
        for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
            assert_eq!(provider.session_exists("/actual-project", id), Some(true));
        }
    });
    for alias in [&undeclared_alias, &foreign.join("projects")] {
        let alias_path = alias.join("-actual-project").join(format!("{id}.jsonl"));
        assert!(crate::session_evidence::record(
            id,
            alias_path.to_str().unwrap()
        ));
        with_context(Some(invocation.clone()), || {
            for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
                assert_eq!(provider.session_exists("/stale-tab-cwd", id), Some(false));
            }
        });
    }
    assert!(crate::session_evidence::record(
        id,
        transcript.to_str().unwrap()
    ));
    for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
        assert_eq!(provider.session_exists("/actual-project", id), Some(true));
        with_context(Some(invocation.clone()), || {
            assert_eq!(provider.session_exists("/stale-tab-cwd", id), Some(true));
            assert_eq!(
                provider.session_exists("/actual-project", "other-session"),
                Some(false)
            );
        });
    }
    #[cfg(unix)]
    std::fs::remove_file(&shared_link).unwrap();
    #[cfg(windows)]
    std::fs::remove_dir(&shared_link).unwrap();
    let decoy = projects.join("decoy");
    let decoy_project = decoy.join("-actual-project");
    std::fs::create_dir_all(&decoy_project).unwrap();
    std::fs::write(decoy_project.join(format!("{id}.jsonl")), "{}\n").unwrap();
    codeterm_util::fs_links::directory_link(&decoy, &shared_link).unwrap();
    assert!(transcript.is_file());
    assert_eq!(
        shared_link.canonicalize().unwrap(),
        decoy.canonicalize().unwrap()
    );
    with_context(Some(invocation), || {
        assert!(provider_account::read_allowed(&transcript));
        assert!(!provider_account::read_allowed(&shared_link));
        for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
            assert_eq!(provider.session_exists("/actual-project", id), None);
            assert_eq!(provider.session_exists("/stale-tab-cwd", id), None);
        }
    });
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
        let reused_start = chrono::Utc
            .with_ymd_and_hms(2026, 8, 1, 5, 30, 0)
            .single()
            .unwrap()
            .timestamp_micros();
        let riga_record_ms = reused_start / 1_000 - 3 * 3_600_000 + 10;
        let fresh_ms = later / 1_000 + 1;
        for (pid, timestamp, modified_ms) in [
            (2860, "Sun Nov  1 05:30:00 2026", start / 1_000 + 1),
            (2861, "Sun Nov  1 06:30:00 2026", fresh_ms),
            (2862, "Sun Nov  1 01:30:00 2026", fresh_ms),
            (2863, "dim.  1 nov. 01:30:00 2026", fresh_ms),
            (2864, "Sat Aug  1 05:30:00 2026", riga_record_ms),
        ] {
            write(
                pid,
                json!({"pid":pid,"sessionId":format!("sid-{pid}"),"procStart":timestamp,"kind":"interactive"}),
            );
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(registry.path().join(format!("{pid}.json")))
                .unwrap();
            file.set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_millis(modified_ms as u64),
            )
            .unwrap();
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
                evidence(vec![process(2864, 1, &reused_start.to_string())]),
                None,
            ),
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
