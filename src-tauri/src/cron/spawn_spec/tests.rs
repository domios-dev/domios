use super::*;
use crate::service::spawn::{AgentSpawnSpec, TabCommand};
use crate::service::spawn_capability::test_backend::registry_with_spawnable;

const TASK: &str = "sweep the release lane and report";

fn inputs(agent: &str) -> CronSpawnInputs {
    CronSpawnInputs {
        job_id: "job-7".into(),
        job_name: "nightly".into(),
        agent: agent.into(),
        model: Some("some-model".into()),
        reasoning_effort: None,
        task_text: TASK.into(),
        role: None,
        cwd: Some("/lane".into()),
        workspace_id: Some("ws-1".into()),
        team_id: Some("team-1".into()),
        creator_tab_id: Some("tab-creator".into()),
        report_target_tab_id: Some("tab-report".into()),
    }
}

/// The spec both cron fire sites built before the routing seam existed. The PTY
/// arm must stay byte-identical to it.
fn legacy_pty_spec() -> SpawnTabSpec {
    SpawnTabSpec {
        workspace_id: Some("ws-1".into()),
        parent_tab_id: None,
        team_id: Some("team-1".into()),
        creator_tab_id: Some("tab-creator".into()),
        detach_from_team: false,
        cwd: Some("/lane".into()),
        title: Some("cron:nightly".into()),
        description: None,
        visibility: TabVisibility::Hidden,
        audience: TabAudience::Automation,
        eager: false,
        command: TabCommand::Agent(AgentSpawnSpec {
            provider: "pty-agent".into(),
            planned_model: None,
            model: Some("some-model".into()),
            task: Some(TASK.into()),
            role: None,
            report_target_tab_id: Some("tab-report".into()),
            spawned_by: Some("cron:job-7".into()),
            ..Default::default()
        }),
    }
}

/// Verdict (a): a job pinned to a provider the registry advertises as a
/// spawnable chat backend routes to the chat runtime and carries the job brief.
#[test]
fn spawn_routing_cron_chat_backend_reaches_the_chat_runtime_with_the_job_brief() {
    let chatbackends = registry_with_spawnable("gamma-shell");
    let spec = build_cron_spawn_spec(&chatbackends, inputs("gamma-shell"));

    assert_eq!(spec.visibility, TabVisibility::Hidden);
    assert_eq!(spec.team_id.as_deref(), Some("team-1"));
    match spec.command {
        TabCommand::ChatBackend {
            plugin_id, config, ..
        } => {
            assert_eq!(plugin_id, "gamma-shell");
            let brief = config["systemPrompt"]
                .as_str()
                .expect("the job brief must reach the chat runtime");
            assert!(
                brief.contains(TASK),
                "the brief this test supplied must survive routing"
            );
        }
        other => panic!("expected the chat runtime for a spawnable backend, got {other:?}"),
    }
}

/// Negative control: with no chat backend registered under that id, the spec is
/// byte-identical to the one the fire sites hand-built before this change.
#[test]
fn spawn_routing_cron_pty_provider_spec_is_byte_identical() {
    let chatbackends = registry_with_spawnable("gamma-shell");
    let spec = build_cron_spawn_spec(&chatbackends, inputs("pty-agent"));

    assert_eq!(
        serde_json::to_value(&spec).unwrap(),
        serde_json::to_value(legacy_pty_spec()).unwrap()
    );
}

#[test]
fn spawn_routing_cron_pty_provider_keeps_the_task_out_of_a_system_prompt() {
    let chatbackends = crate::chatbackend::ChatBackendRegistry::new();
    let spec = build_cron_spawn_spec(&chatbackends, inputs("pty-agent"));
    match spec.command {
        TabCommand::Agent(agent) => assert_eq!(agent.task.as_deref(), Some(TASK)),
        other => panic!("expected the PTY agent runtime, got {other:?}"),
    }
}

#[test]
fn cron_spawn_spec_forwards_reasoning_effort() {
    let chatbackends = crate::chatbackend::ChatBackendRegistry::new();
    let mut inputs = inputs("pty-agent");
    inputs.reasoning_effort = Some("high".into());

    let spec = build_cron_spawn_spec(&chatbackends, inputs);
    match spec.command {
        TabCommand::Agent(agent) => assert_eq!(agent.reasoning_effort.as_deref(), Some("high")),
        other => panic!("expected the PTY agent runtime, got {other:?}"),
    }
}
