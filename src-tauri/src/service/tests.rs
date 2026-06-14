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
    // Lead / SubWorker are gone — the hierarchy is 2-level (Orchestrator + Worker).
    assert!(ServiceContext::parse_role("SubWorker").is_err());
    assert!(ServiceContext::parse_role("Lead").is_err());
    assert!(ServiceContext::parse_role("invalid").is_err());
}

#[test]
fn test_resolve_refs_simple() {
    let mut refs = std::collections::HashMap::new();
    refs.insert("boss".to_string(), "pane-123".to_string());

    let params = serde_json::json!({"parent_pane_id": "$boss", "role": "Worker"});
    let resolved = resolve_refs(&params, &refs);
    assert_eq!(
        resolved.get("parent_pane_id").unwrap().as_str().unwrap(),
        "pane-123"
    );
    assert_eq!(resolved.get("role").unwrap().as_str().unwrap(), "Worker");
}

#[test]
fn test_resolve_refs_no_match() {
    let refs = std::collections::HashMap::new();
    let params = serde_json::json!({"parent_pane_id": "$nonexistent"});
    let resolved = resolve_refs(&params, &refs);
    assert_eq!(
        resolved.get("parent_pane_id").unwrap().as_str().unwrap(),
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
    assert_eq!(params.role.as_deref(), Some("Conductor"));
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
fn test_spawn_agent_params_parent_alias() {
    let json = serde_json::json!({"parent": "pane-abc"});
    let params = SpawnAgentParams::from_json(&json).unwrap();
    assert_eq!(params.parent_pane_id.as_deref(), Some("pane-abc"));
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
