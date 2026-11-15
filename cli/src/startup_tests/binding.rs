use super::*;
use serde_json::{json, Value};

const ROUTING: [&str; 4] = [
    "CODETERM_PLAN_ID",
    "CODETERM_TASK_ID",
    "CODETERM_ATTEMPT_ID",
    "CODETERM_LANE_BASE",
];

struct Environment(Vec<(&'static str, Option<String>)>);

impl Environment {
    fn stale() -> Self {
        let keys = ["CODETERM_TAB_ID"].into_iter().chain(ROUTING);
        let saved = Self(keys.map(|key| (key, std::env::var(key).ok())).collect());
        std::env::set_var("CODETERM_TAB_ID", "tab_bound");
        for key in ROUTING {
            std::env::set_var(key, "stale");
        }
        saved
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct CurrentBinding(Value);

impl client::RpcClient for CurrentBinding {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, client::SocketCallError> {
        assert_eq!(method, "plan.binding.resolve");
        assert_eq!(params, Some(json!({"tab_id":"tab_bound","caller_tab_id":"tab_bound"})));
        Ok(json!({"tab_id":"tab_bound","binding":self.0}))
    }
}

#[test]
fn current_binding_replaces_stale_plan_task_attempt_and_base_before_reparse() {
    let _guard = test_env_lock().lock().unwrap();
    let _env = Environment::stale();
    let args = ["domios", "agent", "report"].map(String::from);
    let mut client = CurrentBinding(json!({
        "plan_id":"current_plan","task_id":"current_task",
        "attempt_id":"current_attempt","base":"current_base"
    }));
    hydrate_tab_task_binding_env(&args, &mut client).unwrap();
    let Commands::Agent { action: AgentAction::Report(report) } = Cli::try_parse_from(&args).unwrap().command else {
        panic!("expected report");
    };
    assert_eq!(report.plan.as_deref(), Some("current_plan"));
    assert_eq!(report.task.as_deref(), Some("current_task"));
    assert_eq!(report.base.as_deref(), Some("current_base"));
    assert_eq!(nonempty_env("CODETERM_ATTEMPT_ID").as_deref(), Some("current_attempt"));

    client.0 = json!({"plan_id":"reparented_team_plan","task_id":null});
    hydrate_tab_task_binding_env(&args, &mut client).unwrap();
    let Commands::Agent { action: AgentAction::Report(report) } = Cli::try_parse_from(&args).unwrap().command else {
        panic!("expected report");
    };
    assert_eq!(report.plan.as_deref(), Some("reparented_team_plan"));
    assert!(report.task.is_none());
    assert!(report.base.is_none());
    assert!(nonempty_env("CODETERM_ATTEMPT_ID").is_none());

    client.0 = Value::Null;
    hydrate_tab_task_binding_env(&args, &mut client).unwrap();
    for key in ROUTING {
        assert!(nonempty_env(key).is_none(), "{key}");
    }
}

#[test]
fn explicit_report_context_wins_over_current_defaults_including_empty_task() {
    let _guard = test_env_lock().lock().unwrap();
    let _env = Environment::stale();
    for task in ["explicit_task", ""] {
        let args = ["domios", "agent", "report", "--plan", "explicit_plan", "--task", task, "--base", "explicit_base"].map(String::from);
        hydrate_tab_task_binding_env(&args, &mut BindingClient).unwrap();
        let Commands::Agent { action: AgentAction::Report(report) } = Cli::try_parse_from(&args).unwrap().command else {
            panic!("expected report");
        };
        assert_eq!(report.plan.as_deref(), Some("explicit_plan"));
        assert_eq!(report.task.as_deref(), Some(task));
        assert_eq!(report.base.as_deref(), Some("explicit_base"));
    }
}

struct Unavailable;

impl client::RpcClient for Unavailable {
    fn call_detailed(
        &mut self,
        _: &str,
        _: Option<Value>,
    ) -> Result<Value, client::SocketCallError> {
        Err(client::SocketCallError::Transport("unavailable".into()))
    }
}

#[test]
fn binding_failure_and_incomplete_response_refuse_without_changing_context() {
    let _guard = test_env_lock().lock().unwrap();
    let _env = Environment::stale();
    let args = ["domios", "agent", "report"].map(String::from);
    assert!(hydrate_tab_task_binding_env(&args, &mut Unavailable).is_err());
    for binding in [json!({}), json!({"plan_id":"plan","task_id":false})] {
        assert!(hydrate_tab_task_binding_env(&args, &mut CurrentBinding(binding)).is_err());
    }
    for key in ROUTING {
        assert_eq!(nonempty_env(key).as_deref(), Some("stale"));
    }
}

#[test]
fn outside_tab_and_mesh_keep_explicit_or_inherited_context_without_local_resolution() {
    let _guard = test_env_lock().lock().unwrap();
    let _env = Environment::stale();
    let args = ["domios", "--mesh=peer", "agent", "report"].map(String::from);
    hydrate_tab_task_binding_env(&args, &mut Unavailable).unwrap();
    std::env::remove_var("CODETERM_TAB_ID");
    let args = ["domios", "agent", "report"].map(String::from);
    hydrate_tab_task_binding_env(&args, &mut Unavailable).unwrap();
    for key in ROUTING {
        assert_eq!(nonempty_env(key).as_deref(), Some("stale"));
    }
}

#[test]
fn only_connected_binding_consumers_refresh_routing() {
    for args in [
        vec!["agent", "spawn"],
        vec!["plan", "task", "extend-files", "--edit-file", "cli/src/main.rs"],
        vec!["plan", "loop", "record", "loop", "--note", "progress"],
    ] {
        let parsed = Cli::try_parse_from(std::iter::once("domios").chain(args)).unwrap();
        assert!(command_consumes_binding(&parsed.command));
    }
    for args in [vec!["docs"], vec!["cli-manifest"], vec!["agent", "startup-ack"], vec!["plan", "skill", "list"]] {
        let parsed = Cli::try_parse_from(std::iter::once("domios").chain(args)).unwrap();
        assert!(!command_consumes_binding(&parsed.command));
    }
}
