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
