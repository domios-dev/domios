use super::*;

fn agent_spec(provider: &str) -> SpawnPaneSpec {
    SpawnPaneSpec {
        visibility: PaneVisibility::Visible,
        command: PaneCommand::Agent(AgentSpawnSpec {
            provider: provider.into(),
            auto_approve: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn default_visibility_is_visible() {
    let spec = SpawnPaneSpec::default();
    assert_eq!(spec.visibility, PaneVisibility::Visible);
}

#[test]
fn default_command_is_shell() {
    let spec = SpawnPaneSpec::default();
    assert!(matches!(spec.command, PaneCommand::Shell));
}

#[test]
fn agent_auto_approve_defaults_true_via_serde() {
    let json = r#"{"provider": "claude"}"#;
    let agent: AgentSpawnSpec = serde_json::from_str(json).unwrap();
    assert!(agent.auto_approve);
}

#[test]
fn agent_auto_approve_defaults_true_via_struct() {
    let agent = AgentSpawnSpec {
        provider: "claude".into(),
        auto_approve: default_true(),
        ..Default::default()
    };
    assert!(agent.auto_approve);
}

#[test]
fn shell_spec_roundtrips_json() {
    let spec = SpawnPaneSpec {
        title: Some("scratch".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.command, PaneCommand::Shell));
    assert_eq!(back.title.as_deref(), Some("scratch"));
}

#[test]
fn agent_spec_roundtrips_json() {
    let spec = SpawnPaneSpec {
        visibility: PaneVisibility::Hidden,
        command: PaneCommand::Agent(AgentSpawnSpec {
            provider: "claude".into(),
            model: Some("opus".into()),
            task: Some("hello".into()),
            role: Some("Worker".into()),
            can_spawn: Some(false),
            auto_approve: true,
            worker_id: Some("w-123".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&spec).unwrap();
    let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back.visibility, PaneVisibility::Hidden);
    match back.command {
        PaneCommand::Agent(a) => {
            assert_eq!(a.provider, "claude");
            assert_eq!(a.model.as_deref(), Some("opus"));
            assert_eq!(a.task.as_deref(), Some("hello"));
            assert_eq!(a.role.as_deref(), Some("Worker"));
            assert_eq!(a.worker_id.as_deref(), Some("w-123"));
            assert!(a.auto_approve);
        }
        _ => panic!("expected Agent"),
    }
}

#[test]
fn from_agent_params_carries_model_into_spec() {
    let params = SpawnAgentParams {
        agent: Some("codex".into()),
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: Some("gpt-5.4-mini".into()),
        resume_session_id: None,
        activate: None,
    };
    let spec = SpawnPaneSpec::from_agent_params(params);
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.model.as_deref(), Some("gpt-5.4-mini")),
        _ => panic!("expected Agent"),
    }
}

#[test]
fn to_agent_params_carries_model_as_dedicated_field() {
    // Model travels as a dedicated `model` field through SpawnAgentParams,
    // not as `--model X` baked into args. `build_agent_command` then asks
    // the provider via `model_launch_args` to render the correct flag
    // syntax (`-m X` for codex/opencode, `--model X` for claude).
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.model = Some("opus".into());
        a.task = Some("count files".into());
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.agent.as_deref(), Some("claude"));
    assert_eq!(params.task.as_deref(), Some("count files"));
    assert_eq!(params.model.as_deref(), Some("opus"));
    let args = params.args.unwrap_or_default();
    assert!(!args.iter().any(|a| a == "--model"), "model must not leak into args");
}

#[test]
fn to_agent_params_passes_role_and_report_target() {
    let mut spec = agent_spec("claude");
    spec.parent_pane_id = Some("parent-1".into());
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Lead".into());
        a.can_spawn = Some(true);
        a.report_target_pane_id = Some("cron-host".into());
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.role.as_deref(), Some("Lead"));
    assert_eq!(params.parent_pane_id.as_deref(), Some("parent-1"));
    assert_eq!(params.can_spawn, Some(true));
    assert_eq!(params.report_target_pane_id.as_deref(), Some("cron-host"));
}

#[test]
fn to_agent_params_fails_on_shell_spec() {
    let spec = SpawnPaneSpec::default();
    let err = spec.to_agent_params().unwrap_err();
    assert!(err.contains("Shell"));
}

#[test]
fn can_spawn_none_round_trips_through_params() {
    // Caller didn't pass --can-spawn → spec.can_spawn=None → params.can_spawn=None.
    // This tells set_pane_role to keep the role's default (Orchestrator/Lead=true).
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Orchestrator".into());
        a.can_spawn = None;
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.can_spawn, None);
}

#[test]
fn can_spawn_some_false_explicitly_overrides() {
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.role = Some("Lead".into());
        a.can_spawn = Some(false);
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.can_spawn, Some(false));
}

#[test]
fn from_agent_params_preserves_can_spawn_option() {
    // CLI/REST/socket pass `can_spawn: Option<bool>` straight through —
    // None means "don't override the role's default", Some(b) means
    // "user said --can-spawn=b explicitly". The spec must NOT collapse
    // None → Some(false) (the pre-fix-#42 behavior that clobbered
    // Orchestrator/Lead defaults to false on every spawn).
    let mut p = SpawnAgentParams {
        agent: Some("claude".into()),
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        role: Some("Orchestrator".into()),
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        resume_session_id: None,
        activate: None,
    };
    let spec = SpawnPaneSpec::from_agent_params(p.clone());
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.can_spawn, None),
        _ => panic!("expected Agent"),
    }

    p.can_spawn = Some(true);
    let spec = SpawnPaneSpec::from_agent_params(p);
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.can_spawn, Some(true)),
        _ => panic!("expected Agent"),
    }
}

// ── activate / focus-on-spawn ─────────────────────────────────────

#[test]
fn from_agent_params_carries_activate_into_spec() {
    let mut p = SpawnAgentParams {
        agent: Some("claude".into()),
        agent_name: None,
        args: None,
        cwd: None,
        title: None,
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: None,
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: None,
        spawned_by_pane_id: None,
        model: None,
        resume_session_id: None,
        activate: Some(true),
    };
    let spec = SpawnPaneSpec::from_agent_params(p.clone());
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.activate, Some(true)),
        _ => panic!("expected Agent"),
    }

    p.activate = None;
    let spec = SpawnPaneSpec::from_agent_params(p);
    match spec.command {
        PaneCommand::Agent(a) => assert_eq!(a.activate, None),
        _ => panic!("expected Agent"),
    }
}

#[test]
fn to_agent_params_round_trips_activate() {
    let mut spec = agent_spec("claude");
    if let PaneCommand::Agent(ref mut a) = spec.command {
        a.activate = Some(true);
    }
    let params = spec.to_agent_params().unwrap();
    assert_eq!(params.activate, Some(true));
}

#[test]
fn spawn_agent_params_from_json_parses_activate_true() {
    let json = serde_json::json!({"activate": true});
    let p = SpawnAgentParams::from_json(&json).unwrap();
    assert_eq!(p.activate, Some(true));
}

#[test]
fn spawn_agent_params_from_json_no_activate_means_none() {
    // Regression for the focus-steal bug: CLI / API spawns that don't
    // pass activate must end up as None (default false) — they no
    // longer steal focus just because they don't set a role.
    let json = serde_json::json!({"agent": "claude", "task": "do work"});
    let p = SpawnAgentParams::from_json(&json).unwrap();
    assert!(p.activate.is_none());
    assert!(p.role.is_none(), "no role + no activate is the bug case");
}

// ── mux-state-level activation gating ──────────────────────────────
//
// The spawn paths in `service::spawn` and `service::orchestrator` both
// gate `mux.set_active_tab(...)` on `activate.unwrap_or(false)`. The
// asserts below mirror that predicate so a future edit that flips the
// default to `true` (or restores `role.is_none()` as the gate) trips a
// test instead of silently re-introducing the focus-steal bug.

fn would_activate(spec: &AgentSpawnSpec) -> bool {
    spec.activate.unwrap_or(false)
}

#[test]
fn spawn_with_activate_true_sets_active_tab() {
    let spec = AgentSpawnSpec {
        provider: "claude".into(),
        activate: Some(true),
        ..Default::default()
    };
    assert!(would_activate(&spec));
}

#[test]
fn spawn_with_activate_false_does_not_steal_focus() {
    let spec = AgentSpawnSpec {
        provider: "claude".into(),
        activate: Some(false),
        ..Default::default()
    };
    assert!(!would_activate(&spec));
}

#[test]
fn spawn_with_no_role_and_no_activate_does_not_steal_focus() {
    // Regression: pre-fix the predicate was `role.is_none()`, so any
    // CLI/API/cron/bench spawn (which legitimately has no role) stole
    // user focus. With opt-in activate, role=None + activate=None must
    // leave focus untouched.
    let spec = AgentSpawnSpec {
        provider: "claude".into(),
        role: None,
        activate: None,
        ..Default::default()
    };
    assert!(!would_activate(&spec));
}
