use super::*;
use crate::service::spawn::{AgentSpawnSpec, TabCommand};
use crate::service::spawn_capability::test_backend::registry_with_spawnable;
use crate::service::spawn_capability::MissingSpawnCapability;

const TASK: &str = "extract entities from the queued item";

fn inputs(agent: &str) -> MemWorkerSpawnInputs {
    MemWorkerSpawnInputs {
        agent_provider: agent.into(),
        model: "some-model".into(),
        task: TASK.into(),
        worker_id: "wk-42".into(),
        spawned_by: "mem:auto".into(),
        intent: "extract".into(),
        instance_cli_path: None,
        inherited_path: std::ffi::OsString::new(),
    }
}

fn worker_cwd() -> std::path::PathBuf {
    crate::paths::data_dir().join("workers")
}

/// The spec `spawn_headless_worker` hand-built before the routing seam existed.
fn legacy_pty_spec() -> SpawnTabSpec {
    SpawnTabSpec {
        cwd: Some(worker_cwd().to_string_lossy().into_owned()),
        visibility: TabVisibility::Hidden,
        audience: TabAudience::Automation,
        title: Some("mem:extract".into()),
        description: None,
        command: TabCommand::Agent(AgentSpawnSpec {
            provider: "pty-agent".into(),
            planned_model: None,
            model: Some("some-model".into()),
            task: Some(TASK.into()),
            worker_id: Some("wk-42".into()),
            spawned_by: Some("mem:auto".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Verdict (b): the memory-worker path refuses a chat backend for the capability
/// it lacks, and never by calling it unknown while the registry advertises it.
#[test]
fn spawn_routing_mem_worker_chat_backend_is_refused_for_the_missing_worker_identity() {
    let chatbackends = registry_with_spawnable("gamma-shell");
    let rejection = build_mem_worker_spawn_spec(&chatbackends, worker_cwd(), inputs("gamma-shell"))
        .expect_err("a chat backend cannot carry the memory-worker identity");

    assert_eq!(
        rejection.missing_capability(),
        Some(MissingSpawnCapability::WorkerIdentityHandoff)
    );
    assert_eq!(rejection.requested(), "gamma-shell");
    assert!(
        !rejection.advertises_requested_as_available(),
        "the refusal must not list the provider it refuses"
    );
}

/// Negative control: the PTY arm is byte-identical to the hand-built spec.
#[test]
fn spawn_routing_mem_worker_pty_provider_spec_is_byte_identical() {
    let chatbackends = registry_with_spawnable("gamma-shell");
    let spec = build_mem_worker_spawn_spec(&chatbackends, worker_cwd(), inputs("pty-agent"))
        .expect("a PTY agent carries the worker identity");

    assert_eq!(
        serde_json::to_value(&spec).unwrap(),
        serde_json::to_value(legacy_pty_spec()).unwrap()
    );
}

#[test]
fn spawn_routing_mem_worker_empty_model_stays_unset() {
    let chatbackends = crate::chatbackend::ChatBackendRegistry::new();
    let spec = build_mem_worker_spawn_spec(
        &chatbackends,
        worker_cwd(),
        MemWorkerSpawnInputs {
            model: String::new(),
            ..inputs("pty-agent")
        },
    )
    .expect("a PTY agent carries the worker identity");

    match spec.command {
        TabCommand::Agent(agent) => assert!(agent.model.is_none()),
        other => panic!("expected the PTY agent runtime, got {other:?}"),
    }
}

#[test]
fn headless_mem_worker_cwd_is_instance_owned() {
    let chatbackends = crate::chatbackend::ChatBackendRegistry::new();
    let spec = build_mem_worker_spawn_spec(&chatbackends, worker_cwd(), inputs("pty-agent"))
        .expect("a PTY agent carries the worker identity");
    let cwd = spec.cwd.expect("headless worker has an explicit cwd");
    let root = crate::paths::data_dir().join("workers");

    assert!(std::path::Path::new(&cwd).starts_with(root));
    assert!(!std::path::Path::new(&cwd).starts_with(std::env::current_dir().unwrap()));
}

#[test]
fn hidden_worker_path_prefers_instance_cli_dir() {
    let chatbackends = crate::chatbackend::ChatBackendRegistry::new();
    let temp = tempfile::tempdir().unwrap();
    let instance_dir = temp.path().join("tabstate").join("bin");
    let prod_dir = temp.path().join("prod").join("bin");
    std::fs::create_dir_all(&instance_dir).unwrap();
    std::fs::create_dir_all(&prod_dir).unwrap();
    let cli_name = format!("codeterm{}", std::env::consts::EXE_SUFFIX);
    let instance_cli = instance_dir.join(&cli_name);
    std::fs::write(&instance_cli, b"").unwrap();
    std::fs::write(prod_dir.join(&cli_name), b"").unwrap();
    let inherited =
        std::env::join_paths([prod_dir.clone(), std::path::PathBuf::from("/usr/bin")]).unwrap();
    let spec = build_mem_worker_spawn_spec(
        &chatbackends,
        worker_cwd(),
        MemWorkerSpawnInputs {
            instance_cli_path: Some(instance_cli.clone().into_os_string()),
            inherited_path: inherited,
            ..inputs("pty-agent")
        },
    )
    .expect("a PTY agent carries the worker identity");
    let TabCommand::Agent(agent) = spec.command else {
        panic!("memory worker uses a PTY agent");
    };
    assert_eq!(
        std::path::PathBuf::from(&agent.extra_env["CODETERM_CLI_PATH"]),
        instance_cli
    );
    let path =
        std::env::split_paths(std::ffi::OsStr::new(&agent.extra_env["PATH"])).collect::<Vec<_>>();
    assert_eq!(path.first(), Some(&instance_dir));
    assert!(path.iter().skip(1).any(|entry| entry == &prod_dir));
    let resolved_cli = path
        .iter()
        .map(|directory| directory.join(&cli_name))
        .find(|candidate| candidate.is_file())
        .expect("a codeterm executable resolves on the configured PATH");
    assert_eq!(resolved_cli, instance_cli);
}
