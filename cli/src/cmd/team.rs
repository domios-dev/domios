use serde_json::{json, Value};

use crate::args::team::TeamAction;
use crate::client::{RpcClient, SocketCallError};

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TeamAction) {
    if let TeamAction::Pair {
        orchestrator,
        planner,
        workspace,
        title,
        acknowledge_leaf,
    } = action
    {
        handle_pair(
            client,
            json_output,
            &orchestrator,
            &planner,
            workspace,
            title,
            acknowledge_leaf,
        );
        return;
    }
    let caller_tab_id = (!client.is_remote())
        .then(|| std::env::var("CODETERM_TAB_ID").ok())
        .flatten();
    let result = match action {
        TeamAction::List => client.call_detailed("team.list", None),
        TeamAction::Show { team_id } => {
            client.call_detailed("team.show", Some(json!({ "team_id": team_id })))
        }
        TeamAction::Rename { team_id, name } => client.call_detailed(
            "team.rename",
            Some(json!({
                "team_id": team_id,
                "name": name,
                "caller_tab_id": caller_tab_id,
            })),
        ),
        TeamAction::Close { team_id } => client.call_detailed(
            "team.close",
            Some(close_team_params(&team_id, caller_tab_id.as_deref())),
        ),
        TeamAction::AdoptPlan { team_id, plan_id } => {
            adopt_plan_params(team_id, plan_id, caller_tab_id.as_deref())
                .map_err(SocketCallError::Transport)
                .and_then(|params| client.call_detailed("team.adopt_plan", Some(params)))
        }
        TeamAction::AdoptAgent { team_id, tab_id } => {
            adopt_agent_params(team_id, tab_id, caller_tab_id.as_deref())
                .map_err(SocketCallError::Transport)
                .and_then(|params| client.call_detailed("team.adopt_agent", Some(params)))
        }
        TeamAction::AttachCartographer {
            team_id,
            provider,
            model,
        } => client.call_detailed(
            "team.add_member",
            Some(cartographer_params(
                team_id,
                provider,
                model,
                caller_tab_id.as_deref(),
            )),
        ),
        TeamAction::Pair { .. } => unreachable!("pair returns before the socket verbs"),
    };
    match result {
        Ok(value) if json_output => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())
        ),
        Ok(value) => print_human(&value),
        Err(error) => {
            crate::cmd::model_error::exit_with_socket_error(json_output, error);
        }
    }
}

/// Model catalogue refusals add the catalogue refresh command; other errors
/// keep the daemon's message intact with the CLI's standard error prefix.
#[cfg(test)]
pub(crate) fn render_daemon_error(error: &SocketCallError) -> String {
    crate::cmd::model_error::render_socket_error(error)
}

fn handle_pair(
    client: &mut dyn RpcClient,
    json_output: bool,
    orchestrator: &str,
    planner: &str,
    workspace: Option<String>,
    title: Option<String>,
    acknowledge_leaf: bool,
) {
    let params = pair_params(orchestrator, planner, workspace, title, acknowledge_leaf);
    match client.call_detailed("orchestration.new", Some(params)) {
        Ok(value) if json_output => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())
        ),
        Ok(value) => print_pair(&value),
        Err(error) => {
            crate::cmd::model_error::exit_with_socket_error(json_output, error);
        }
    }
}

/// Splits a `provider[:model]` spec. A trailing empty model (`"claude:"`) is
/// treated the same as no model — let the provider descriptor default apply.
pub(crate) fn split_provider_model(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        Some((provider, model)) if !model.trim().is_empty() => {
            (provider.trim().to_string(), Some(model.trim().to_string()))
        }
        _ => (
            spec.split(':').next().unwrap_or(spec).trim().to_string(),
            None,
        ),
    }
}

pub(crate) fn pair_params(
    orchestrator: &str,
    planner: &str,
    workspace: Option<String>,
    title: Option<String>,
    acknowledge_leaf: bool,
) -> Value {
    let (orchestrator_agent, orchestrator_model) = split_provider_model(orchestrator);
    let (planner_agent, planner_model) = split_provider_model(planner);

    let mut params = json!({
        "orchestrator_agent": orchestrator_agent,
        "planner_agent": planner_agent,
    });
    if let Some(m) = orchestrator_model {
        params["orchestrator_model"] = json!(m);
    }
    if let Some(m) = planner_model {
        params["planner_model"] = json!(m);
    }
    if let Some(w) = workspace {
        params["workspace_id"] = json!(w);
    }
    if let Some(t) = title {
        params["title"] = json!(t);
    }
    if let Ok(spawner) = std::env::var("CODETERM_TAB_ID") {
        if !spawner.is_empty() {
            params["spawned_by_tab_id"] = json!(spawner);
        }
    }
    if acknowledge_leaf {
        params["leaf_creation_acknowledged"] = json!(true);
    }
    params
}

fn print_pair(value: &Value) {
    let plan_id = value.get("plan_id").and_then(Value::as_str).unwrap_or("?");
    let orch_tab = value
        .get("orchestrator_tab_id")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let planner_tab = value
        .get("planner_tab_id")
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("Plan: {plan_id}");
    println!("Orchestrator: Tab {orch_tab}");
    println!("Planner: Tab {planner_tab}");
    if let Some(warning) = value.get("warning").and_then(Value::as_str) {
        eprintln!("Warning: {warning}");
    }
}

fn adopt_plan_params(
    team_id: String,
    plan_id: String,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    let caller_tab_id = caller_tab_id
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| {
            "Team plan adoption requires CODETERM_TAB_ID from the calling Team manager".to_string()
        })?;
    Ok(json!({
        "team_id": team_id,
        "plan_id": plan_id,
        "caller_tab_id": caller_tab_id,
    }))
}

fn close_team_params(team_id: &str, caller_tab_id: Option<&str>) -> Value {
    json!({
        "team_id": team_id,
        "force": true,
        "confirm": team_id,
        "caller_tab_id": caller_tab_id,
    })
}

fn adopt_agent_params(
    team_id: String,
    tab_id: String,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    let caller_tab_id = caller_tab_id
        .filter(|caller| !caller.trim().is_empty())
        .ok_or_else(|| {
            "Team agent adoption requires CODETERM_TAB_ID from a live Team member".to_string()
        })?;
    Ok(json!({
        "team_id": team_id,
        "tab_id": tab_id,
        "caller_tab_id": caller_tab_id,
    }))
}

fn cartographer_params(
    team_id: String,
    provider: String,
    model: Option<String>,
    caller_tab_id: Option<&str>,
) -> Value {
    let mut params = json!({
        "team_id": team_id,
        "provider": provider,
        "role": "cartographer",
    });
    if let Some(model) = model {
        params["model"] = json!(model);
    }
    if let Some(caller_tab_id) = caller_tab_id.filter(|tab_id| !tab_id.trim().is_empty()) {
        params["caller_tab_id"] = json!(caller_tab_id);
    }
    params
}

fn cartographer_attachment_summary(value: &Value) -> Option<String> {
    let tab_id = value.get("tab_id").and_then(Value::as_str)?;
    let team_id = value
        .get("team")
        .and_then(|team| team.get("id"))
        .and_then(Value::as_str)?;
    let action = if value.get("reused").and_then(Value::as_bool) == Some(true) {
        "Reused existing"
    } else {
        "Attached"
    };
    Some(format!(
        "{action} Cartographer Manager as Tab {tab_id} in Team {team_id}."
    ))
}

fn print_human(value: &Value) {
    if let Some(warning) = value.get("warning").and_then(Value::as_str) {
        eprintln!("Warning: {warning}");
    }
    if let Some(teams) = value.get("teams").and_then(Value::as_array) {
        if teams.is_empty() {
            println!("No Teams.");
            return;
        }
        for team in teams {
            print_team(team);
        }
        return;
    }
    if let (Some(team_id), Some(tab_id)) = (
        value.get("team_id").and_then(Value::as_str),
        value.get("tab_id").and_then(Value::as_str),
    ) {
        println!("Adopted Tab {tab_id} into Team {team_id}.");
        return;
    }
    if let Some(summary) = cartographer_attachment_summary(value) {
        println!("{summary}");
        return;
    }
    if let Some(team) = value.get("team") {
        print_team(team);
        if let Some(inventory) = value.get("inventory") {
            print_close_inventory(inventory);
        }
        print_members(
            value
                .get("members")
                .and_then(Value::as_array)
                .map(Vec::as_slice),
        );
        return;
    }
    let team_id = value.get("team_id").and_then(Value::as_str).unwrap_or("?");
    let plan_id = value
        .get("plan")
        .and_then(|plan| plan.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("Adopted plan {plan_id} into Team {team_id}.");
}

fn print_close_inventory(inventory: &Value) {
    for record in inventory
        .get("removed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        println!(
            "Removed lane path={} branch={}",
            record
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("<none>"),
            record
                .get("branch")
                .and_then(Value::as_str)
                .unwrap_or("<none>")
        );
    }
    for record in inventory
        .get("kept")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        println!(
            "Kept lane path={} branch={} reason={}",
            record
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("<none>"),
            record
                .get("branch")
                .and_then(Value::as_str)
                .unwrap_or("<none>"),
            record
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    for branch in inventory
        .get("orphan_branches")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(branch) = branch.as_str() {
            println!("Orphan lane branch={branch}");
        }
    }
}

fn print_members(members: Option<&[Value]>) {
    let Some(members) = members else { return };
    if members.is_empty() {
        println!("  (no member tabs)");
        return;
    }
    for member in members {
        let tab_id = member.get("tab_id").and_then(Value::as_str).unwrap_or("?");
        let title = member.get("title").and_then(Value::as_str).unwrap_or("");
        let role = team_member_role_label(member);
        println!("  {tab_id}\t{role}\t{title}");
    }
}

fn team_member_role_label(member: &Value) -> String {
    if member.get("role_type").and_then(Value::as_str) == Some("Manager")
        && member.get("role_profile").and_then(Value::as_str) == Some("cartographer")
    {
        return "Manager (Cartographer)".into();
    }
    member
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string()
}

fn print_team(team: &Value) {
    let id = team.get("id").and_then(Value::as_str).unwrap_or("?");
    let name = team
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unnamed");
    let lifecycle = team
        .get("lifecycle")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!("{id}\t{name}\t{lifecycle}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopt_plan_params_require_nonempty_caller() {
        let params = adopt_plan_params("team".into(), "plan".into(), Some("manager")).unwrap();
        assert_eq!(params["team_id"], "team");
        assert_eq!(params["plan_id"], "plan");
        assert_eq!(params["caller_tab_id"], "manager");

        for caller in [None, Some(""), Some("  ")] {
            let error = adopt_plan_params("team".into(), "plan".into(), caller).unwrap_err();
            assert!(error.contains("CODETERM_TAB_ID"));
        }
    }

    #[test]
    fn adopt_agent_params_require_nonempty_caller() {
        let params =
            adopt_agent_params("team".into(), "tab_seven".into(), Some("manager")).unwrap();
        assert_eq!(params["team_id"], "team");
        assert_eq!(params["tab_id"], "tab_seven");
        assert_eq!(params["caller_tab_id"], "manager");

        for caller in [None, Some(""), Some("  ")] {
            let error = adopt_agent_params("team".into(), "tab_seven".into(), caller).unwrap_err();
            assert!(error.contains("CODETERM_TAB_ID"));
        }
    }

    #[test]
    fn cartographer_params_use_the_shared_team_member_contract() {
        assert_eq!(
            cartographer_params(
                "team_a".into(),
                "provider-neutral".into(),
                Some("model-1".into()),
                Some("manager-tab"),
            ),
            json!({
                "team_id": "team_a",
                "provider": "provider-neutral",
                "role": "cartographer",
                "model": "model-1",
                "caller_tab_id": "manager-tab",
            })
        );
    }

    #[test]
    fn close_team_params_echo_the_target_for_headerless_human_confirmation() {
        assert_eq!(
            close_team_params("team-1", None),
            json!({
                "team_id": "team-1",
                "force": true,
                "confirm": "team-1",
                "caller_tab_id": null,
            })
        );
        assert_eq!(
            close_team_params("team-1", Some("manager-1"))["caller_tab_id"],
            "manager-1",
        );
    }

    #[test]
    fn cartographer_attachment_summary_distinguishes_reuse_from_creation() {
        let base = json!({
            "tab_id": "cartographer-tab",
            "team": { "id": "team_a" },
            "reused": false,
        });
        assert_eq!(
            cartographer_attachment_summary(&base).as_deref(),
            Some("Attached Cartographer Manager as Tab cartographer-tab in Team team_a.")
        );
        assert_eq!(
            cartographer_attachment_summary(
                &json!({ "reused": true, "tab_id": "cartographer-tab", "team": { "id": "team_a" } })
            )
            .as_deref(),
            Some("Reused existing Cartographer Manager as Tab cartographer-tab in Team team_a.")
        );
    }

    #[test]
    fn team_show_labels_cartographer_as_an_attached_manager() {
        assert_eq!(
            team_member_role_label(&json!({
                "role": "cartographer",
                "role_type": "Manager",
                "role_profile": "cartographer",
            })),
            "Manager (Cartographer)"
        );
        assert_eq!(
            team_member_role_label(&json!({ "role": "planner" })),
            "planner"
        );
    }

    #[test]
    fn general_agent_team_guard_refusal_reaches_the_cli_unedited() {
        // Opaque on purpose: the CLI must forward whatever the daemon says, so
        // a reworded refusal here would hide the violation id and the fix.
        let refusal = "some.violation.id: Tab tab_x is refused. Do this instead:\n  \
                       codeterm team pair --orchestrator alpha --planner beta --workspace ws_7";
        let rendered = render_daemon_error(&SocketCallError::Transport(refusal.into()));
        assert!(rendered.ends_with(refusal), "{rendered}");
        assert_eq!(rendered.len(), refusal.len() + "Error: ".len());
    }

    #[test]
    fn general_agent_team_guard_pair_params_carry_the_workspace_the_refusal_teaches() {
        let params = pair_params("alpha", "beta", Some("ws_7".into()), None, false);
        assert_eq!(params["orchestrator_agent"], "alpha");
        assert_eq!(params["planner_agent"], "beta");
        assert_eq!(params["workspace_id"], "ws_7");
    }

    /// codeterm#400: the CLI must not silently declare leaf intent — the flag
    /// carries through to the daemon only when the caller actually passed it.
    #[test]
    fn pair_params_omit_leaf_declaration_unless_the_flag_was_passed() {
        let params = pair_params("alpha", "beta", None, None, false);
        assert!(params.get("leaf_creation_acknowledged").is_none());

        let params = pair_params("alpha", "beta", None, None, true);
        assert_eq!(params["leaf_creation_acknowledged"], true);
    }

    #[test]
    fn splits_provider_and_model() {
        assert_eq!(
            split_provider_model("claude:fable"),
            ("claude".to_string(), Some("fable".to_string()))
        );
    }

    #[test]
    fn provider_only_has_no_model() {
        assert_eq!(split_provider_model("codex"), ("codex".to_string(), None));
    }

    #[test]
    fn trailing_colon_with_no_model_falls_back_to_provider_only() {
        assert_eq!(
            split_provider_model("claude:"),
            ("claude".to_string(), None)
        );
    }
}
