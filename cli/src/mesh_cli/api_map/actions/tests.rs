use super::agent_to_api;
use crate::{AgentAction, Commands};
use clap::Parser;

const ACTIONS: &[(&str, &[&str], Option<(&str, &str, bool)>)] = &[
    (
        "set-orchestrator",
        &["set-orchestrator", "--tab", "peer-tab"],
        None,
    ),
    (
        "link",
        &["link", "--parent-tab", "parent-tab", "--tab", "peer-tab"],
        None,
    ),
    ("unlink", &["unlink", "--tab", "peer-tab"], None),
    (
        "allow-spawn",
        &["allow-spawn", "--tab", "peer-tab"],
        None,
    ),
    ("report", &["report", "status update"], None),
    (
        "ack",
        &["ack", "--tab", "peer-tab", "--plan", "plan-1", "--task", "task-1"],
        None,
    ),
    ("startup-ack", &["startup-ack"], None),
    (
        "startup-hold",
        &["startup-hold", "--tab", "peer-tab", "--attempt", "attempt-1"],
        None,
    ),
    ("ledger", &["ledger", "--tab", "peer-tab"], None),
    ("compact", &["compact", "--tab", "peer-tab"], None),
    (
        "restart",
        &["restart", "--tab", "004c7c5c", "--force", "--grace-secs", "17"],
        Some(("POST", "/api/tabs/004c7c5c/agent/restart", true)),
    ),
    (
        "model",
        &["model", "gpt-5", "--tab", "peer-tab"],
        None,
    ),
    (
        "providers",
        &["providers"],
        Some(("GET", "/api/agents/providers", false)),
    ),
    (
        "models",
        &["models", "claude"],
        Some(("GET", "/api/agents/providers", false)),
    ),
    (
        "spawn",
        &["spawn", "claude", "--cwd", "/peer/workspace", "--task", "inspect"],
        Some(("POST", "/api/agents/spawn", true)),
    ),
];

struct ScopedTabId(Option<std::ffi::OsString>);

impl ScopedTabId {
    fn set(value: &str) -> Self {
        let previous = std::env::var_os("CODETERM_TAB_ID");
        std::env::set_var("CODETERM_TAB_ID", value);
        Self(previous)
    }
}

impl Drop for ScopedTabId {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("CODETERM_TAB_ID", value),
            None => std::env::remove_var("CODETERM_TAB_ID"),
        }
    }
}

fn parse_action(args: &[&str]) -> AgentAction {
    let mut argv = vec!["codeterm", "agent"];
    argv.extend_from_slice(args);
    match <crate::Cli as Parser>::try_parse_from(argv)
        .expect("agent action arguments parse")
        .command
    {
        Commands::Agent { action } => action,
        _ => unreachable!("agent arguments parse as an agent command"),
    }
}

#[test]
fn every_agent_action_has_an_explicit_mesh_mapping() {
    for &(name, args, expected) in ACTIONS {
        let action = parse_action(args);
        let (method, path, body) = agent_to_api(&action);
        match expected {
            Some((expected_method, expected_path, has_body)) => {
                assert_eq!(method, expected_method, "{name} method");
                assert_eq!(path, expected_path, "{name} path");
                assert_eq!(body.is_some(), has_body, "{name} body presence");
            }
            None => {
                assert_eq!(
                    method,
                    crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH,
                    "{name}"
                );
                assert!(path.starts_with("agent "), "{name} has a clear refusal: {path}");
                assert!(body.is_none(), "{name} refusal has no request body");
            }
        }
    }
}

#[test]
fn agent_restart_mesh_preserves_explicit_tab() {
    let action = parse_action(&[
        "restart",
        "--tab",
        "004c7c5c",
        "--force",
        "--grace-secs",
        "17",
    ]);
    let (method, path, body) = agent_to_api(&action);

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/004c7c5c/agent/restart");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body.expect("restart carries its options"))
            .unwrap(),
        serde_json::json!({"force": true, "grace_secs": 17})
    );
}

#[test]
fn restart_without_tab_over_mesh_is_refused_even_with_local_tab_environment() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let _tab_id = ScopedTabId::set("local-tab");
    let action = parse_action(&["restart"]);
    let (method, reason, body) = agent_to_api(&action);

    assert_eq!(method, crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("requires an explicit --tab"), "{reason}");
    assert!(body.is_none());
}
