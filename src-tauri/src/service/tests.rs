//! Tests for the service layer (types, helpers, serialization).

use super::*;
use crate::multiplexer::types::AgentRole;

#[test]
fn test_parse_role() {
    assert!(matches!(
        ServiceContext::parse_role("Conductor"),
        Ok(AgentRole::Orchestrator)
    ));
    assert!(matches!(
        ServiceContext::parse_role("Worker"),
        Ok(AgentRole::Worker)
    ));
    assert!(matches!(
        ServiceContext::parse_role("Manager"),
        Ok(AgentRole::Manager)
    ));
    // Lead / SubWorker are gone — the hierarchy is 2-level (Orchestrator + Worker).
    assert!(ServiceContext::parse_role("SubWorker").is_err());
    assert!(ServiceContext::parse_role("Lead").is_err());
    assert!(ServiceContext::parse_role("invalid").is_err());
}

#[test]
fn test_resolve_refs_simple() {
    let mut refs = std::collections::HashMap::new();
    refs.insert("boss".to_string(), "tab-123".to_string());

    let params = serde_json::json!({"parent_tab_id": "$boss", "role": "Worker"});
    let resolved = resolve_refs(&params, &refs);
    assert_eq!(
        resolved.get("parent_tab_id").unwrap().as_str().unwrap(),
        "tab-123"
    );
    assert_eq!(resolved.get("role").unwrap().as_str().unwrap(), "Worker");
}

#[test]
fn test_resolve_refs_no_match() {
    let refs = std::collections::HashMap::new();
    let params = serde_json::json!({"parent_tab_id": "$nonexistent"});
    let resolved = resolve_refs(&params, &refs);
    assert_eq!(
        resolved.get("parent_tab_id").unwrap().as_str().unwrap(),
        "$nonexistent"
    );
}

#[test]
fn test_resolve_refs_nested() {
    let mut refs = std::collections::HashMap::new();
    refs.insert("a".to_string(), "id-1".to_string());

    let params = serde_json::json!({"nested": {"ref": "$a"}, "arr": ["$a", "literal"]});
    let resolved = resolve_refs(&params, &refs);
    assert_eq!(resolved["nested"]["ref"].as_str().unwrap(), "id-1");
    assert_eq!(resolved["arr"][0].as_str().unwrap(), "id-1");
    assert_eq!(resolved["arr"][1].as_str().unwrap(), "literal");
}

#[test]
fn test_spawn_agent_params_from_json() {
    let json = serde_json::json!({
        "agent": "claude",
        "agent_name": "architect",
        "role": "Conductor",
        "can_spawn": true,
        "task": "do something",
        "args": ["--verbose"],
    });
    let params = SpawnAgentParams::from_json(&json).unwrap();
    assert_eq!(params.agent.as_deref(), Some("claude"));
    assert_eq!(params.agent_name.as_deref(), Some("architect"));
    assert_eq!(params.role.as_deref(), Some("Orchestrator"));
    assert_eq!(params.can_spawn, Some(true));
    assert_eq!(params.task.as_deref(), Some("do something"));
    assert_eq!(params.args.as_ref().unwrap().len(), 1);
}

#[test]
fn test_spawn_agent_params_from_json_minimal() {
    let json = serde_json::json!({});
    let params = SpawnAgentParams::from_json(&json).unwrap();
    assert!(params.agent.is_none());
    assert!(params.role.is_none());
    assert!(params.task.is_none());
}

#[test]
fn spawn_payload_cannot_override_global_agent_access() {
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "skip_permissions": true,
    }))
    .unwrap();
    assert_eq!(params.skip_permissions, None);
    assert!(params.permission_bypass_authorization.is_none());
}

#[test]
fn spawn_agent_params_from_json_treats_empty_role_fields_as_absent() {
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "role": "  ",
        "role_profile": "",
    }))
    .unwrap();
    assert_eq!(params.agent.as_deref(), Some("claude"));
    assert!(params.role.is_none());
    assert!(params.role_profile.is_none());
}

#[test]
fn spawn_agent_params_from_json_maps_planner_to_manager_profile() {
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "role": "Planner",
        "plan_id": "pl_test",
    }))
    .unwrap();
    assert_eq!(params.role.as_deref(), Some("Manager"));
    assert_eq!(params.role_profile.as_deref(), Some("planner"));
    assert_eq!(params.plan_id.as_deref(), Some("pl_test"));
}

#[test]
fn spawn_agent_params_from_json_maps_watcher_to_manager_profile() {
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "lmstudio",
        "role": "Watcher",
        "charter": "Observe.",
        "tick_interval_secs": 90,
        "nudge_cooldown_secs": 300,
        "max_actions_per_tick": 2,
    }))
    .unwrap();
    assert_eq!(params.role.as_deref(), Some("Manager"));
    assert_eq!(params.role_profile.as_deref(), Some("watcher"));
    assert_eq!(params.charter.as_deref(), Some("Observe."));
    assert_eq!(params.tick_interval_secs, Some(90));
    assert_eq!(params.nudge_cooldown_secs, Some(300));
    assert_eq!(params.max_actions_per_tick, Some(2));
}

#[test]
fn spawn_agent_params_from_json_maps_plan_task() {
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "role": "Worker",
        "plan_id": "pl_test",
        "plan_task": "my-task",
    }))
    .unwrap();
    assert_eq!(params.role.as_deref(), Some("Worker"));
    assert_eq!(params.plan_id.as_deref(), Some("pl_test"));
    assert_eq!(params.plan_task.as_deref(), Some("my-task"));
}

#[test]
fn manager_requires_known_profile() {
    let missing = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "role": "Manager",
    }))
    .unwrap_err();
    assert!(missing.contains("Manager requires a known role_profile"));

    let unknown = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "role": "Manager",
        "role_profile": "reviewer",
    }))
    .unwrap_err();
    assert!(unknown.contains("Manager requires a known role_profile"));
}

#[test]
fn unknown_role_is_rejected() {
    let err = SpawnAgentParams::from_json(&serde_json::json!({
        "agent": "claude",
        "role": "Lead",
    }))
    .unwrap_err();
    assert!(err.contains("Invalid role: Lead"));
    assert!(err.contains("Planner"));
    assert!(err.contains("Watcher"));
}

#[test]
fn test_spawn_agent_params_parent_alias() {
    let json = serde_json::json!({"parent": "tab-abc"});
    let params = SpawnAgentParams::from_json(&json).unwrap();
    assert_eq!(params.parent_tab_id.as_deref(), Some("tab-abc"));
}

#[test]
fn test_spawn_agent_params_normalizes_msys_converted_title() {
    let json = serde_json::json!({
        "title": "C:/Program Files/Git/web 404 investigator"
    });
    let params = SpawnAgentParams::from_json(&json).unwrap();
    assert_eq!(params.title.as_deref(), Some("web 404 investigator"));
}

#[test]
fn tab_description_validation_accepts_200_and_rejects_201() {
    let ok = "x".repeat(crate::service::MAX_TAB_DESCRIPTION_CHARS);
    let too_long = "x".repeat(crate::service::MAX_TAB_DESCRIPTION_CHARS + 1);

    assert_eq!(
        crate::service::normalize_tab_description(&ok).unwrap(),
        Some(ok)
    );
    assert!(crate::service::normalize_tab_description(&too_long).is_err());
    assert_eq!(
        crate::service::normalize_tab_description("  \t  ").unwrap(),
        None
    );
    assert_eq!(
        crate::service::normalize_tab_description("  running tests  ").unwrap(),
        Some("running tests".into())
    );
}

#[test]
fn spawn_agent_params_from_json_validates_description() {
    let ok = "x".repeat(crate::service::MAX_TAB_DESCRIPTION_CHARS);
    let params = SpawnAgentParams::from_json(&serde_json::json!({
        "description": format!("  {}  ", ok)
    }))
    .unwrap();
    assert_eq!(params.description.as_deref(), Some(ok.as_str()));

    let too_long = "x".repeat(crate::service::MAX_TAB_DESCRIPTION_CHARS + 1);
    let err = SpawnAgentParams::from_json(&serde_json::json!({
        "description": too_long
    }))
    .unwrap_err();
    assert!(err.contains("description must be 200 characters or fewer"));
}

#[test]
fn test_json_str() {
    let v = serde_json::json!({"name": "test", "count": 5});
    assert_eq!(json_str(&v, "name"), Some("test".to_string()));
    assert_eq!(json_str(&v, "count"), None); // not a string
    assert_eq!(json_str(&v, "missing"), None);
}

#[test]
fn test_batch_op_deserialize() {
    let json = r#"{"method":"spawn","params":{"agent":"claude"},"id":"cond"}"#;
    let op: BatchOp = serde_json::from_str(json).unwrap();
    assert_eq!(op.method, "spawn");
    assert_eq!(op.id, Some("cond".to_string()));
}

#[test]
fn test_batch_op_no_id() {
    let json = r#"{"method":"send","params":{"text":"hello"}}"#;
    let op: BatchOp = serde_json::from_str(json).unwrap();
    assert_eq!(op.method, "send");
    assert!(op.id.is_none());
}

#[test]
fn coalesce_snapshot_reflects_latest_mutation_in_window() {
    use crate::agent::status::TabStatusTracker;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::state::MultiplexerState;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    let mux: MuxState = Arc::new(TimedRwLock::new("mux", MultiplexerState::empty()));
    let agent_tracker: AgentTrackerState =
        Arc::new(TimedMutex::new("agent_tracker", TabStatusTracker::new()));
    let pending = Arc::new(AtomicBool::new(true));
    let agent_registry = Arc::new(crate::agent::registry::AgentRegistry::new());

    let mux_clone = mux.clone();
    let agent_tracker_clone = agent_tracker.clone();
    let agent_registry_clone = agent_registry.clone();
    let pending_clone = pending.clone();
    let handle = std::thread::spawn(move || {
        ServiceContext::coalesce_snapshot(
            &mux_clone,
            &agent_tracker_clone,
            &agent_registry_clone,
            &pending_clone,
            Duration::from_millis(80),
        )
    });

    // Both mutations land well inside the single coalesce window.
    mux.write().create_workspace("first".to_string(), None);
    mux.write().create_workspace("second".to_string(), None);

    let snapshot = handle.join().unwrap();

    // the latest being "second".
    assert_eq!(
        snapshot.workspaces.len(),
        2,
        "snapshot reflects both in-window mutations"
    );
    assert_eq!(snapshot.workspaces.last().unwrap().name, "second");
    // Pending flag cleared so a mutation after the window starts a fresh one.
    assert!(!pending.load(Ordering::Acquire));
}

#[test]
fn focus_tab_hot_path_does_not_refresh_agent_model_inline() {
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::state::MultiplexerState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let mux: MuxState = Arc::new(TimedRwLock::new("mux", MultiplexerState::empty()));
    let tab_id = {
        let mut mux = mux.write();
        let workspace_id = mux.create_workspace("Test".into(), None).id;
        mux.create_tab(&workspace_id).unwrap()
    };
    let emitted = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let scheduled = Arc::new(AtomicUsize::new(0));

    crate::service::tab::focus_tab_hot_path(
        &mux,
        &tab_id,
        {
            let emitted = emitted.clone();
            move |focused| emitted.lock().push(focused.to_string())
        },
        {
            let scheduled = scheduled.clone();
            move || {
                scheduled.fetch_add(1, Ordering::SeqCst);
            }
        },
    )
    .unwrap();

    assert_eq!(mux.read().get_focused_tab_id(), Some(tab_id.as_str()));
    assert_eq!(*emitted.lock(), vec![tab_id.clone()]);
    assert_eq!(scheduled.load(Ordering::SeqCst), 1);
}
