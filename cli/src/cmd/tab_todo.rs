use std::collections::HashSet;

use codeterm_types::agent::{
    TabTodoChecklist, TabTodoDraft, TabTodoItem, TabTodoLink, TabTodoState, MAX_TAB_TODO_ITEMS,
    MAX_TAB_TODO_TITLE_CHARS,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::args::tab_todo::TabTodoAction;
use crate::client::RpcClient;

const MAX_TODO_INPUT_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum AgentTodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentTodoWriteItem {
    content: String,
    status: AgentTodoStatus,
    #[serde(rename = "activeForm")]
    active_form: String,
    #[serde(default)]
    link: Option<String>,
}

pub fn handle_tab(client: &mut dyn RpcClient, json_output: bool, action: TabTodoAction) {
    match execute_tab(client, action) {
        Ok(value) => print_value(&value, json_output),
        Err(error) => crate::cmd::print::exit_with_cli_error(
            json_output,
            &crate::cmd::print::CliError::new(error),
        ),
    }
}

pub fn handle_agent(client: &mut dyn RpcClient, json_output: bool, write_from_pad: Option<String>) {
    let result = match write_from_pad {
        Some(path) => write_agent_todos(client, &path),
        None => list_agent_todos(client),
    };
    match result {
        Ok(value) => print_value(&value, json_output),
        Err(error) => crate::cmd::print::exit_with_cli_error(
            json_output,
            &crate::cmd::print::CliError::new(error),
        ),
    }
}

fn execute_tab(client: &mut dyn RpcClient, action: TabTodoAction) -> Result<Value, String> {
    match action {
        TabTodoAction::Init {
            tab,
            from_pad,
            from_file,
        } => {
            let actor = required_current_tab()?;
            let criteria: Vec<String> = parse_source(from_pad.as_deref(), from_file.as_deref())?;
            client.call(
                "tab.todo.create",
                Some(json!({
                    "tab_id": tab,
                    "caller_tab_id": actor,
                    "acceptance_criteria": criteria,
                })),
            )
        }
        TabTodoAction::List { tab } => {
            let target = target_tab(tab)?;
            let actor = current_tab();
            let method = crate::cmd::tab::todo_list_rpc_method(&target, actor.as_deref());
            let mut params = json!({ "tab_id": target });
            if let Some(actor) = actor {
                params["caller_tab_id"] = json!(actor);
            }
            client.call(method, Some(params))
        }
        TabTodoAction::Complete { item_id, tab } => {
            let target = target_tab(tab)?;
            let actor = current_tab();
            let checklist = required_checklist(client, &target, actor.as_deref())?;
            let item = checklist
                .items
                .iter()
                .find(|item| item.id == item_id)
                .ok_or_else(|| format!("checklist item not found: {item_id}"))?;
            call_state(
                client,
                &target,
                actor.as_deref(),
                &checklist,
                item,
                TabTodoState::Done,
            )
        }
        TabTodoAction::Append {
            tab,
            generation,
            from_pad,
            from_file,
        } => {
            let actor = required_current_tab()?;
            let items: Vec<TabTodoDraft> = parse_source(from_pad.as_deref(), from_file.as_deref())?;
            client.call(
                "tab.todo.append",
                Some(json!({
                    "tab_id": tab,
                    "caller_tab_id": actor,
                    "generation": generation,
                    "items": items,
                })),
            )
        }
        TabTodoAction::Amend {
            item_id,
            tab,
            from_pad,
            from_file,
        } => {
            let actor = required_current_tab()?;
            let checklist = required_checklist(client, &tab, Some(&actor))?;
            let item = checklist
                .items
                .iter()
                .find(|item| item.id == item_id)
                .ok_or_else(|| format!("checklist item not found: {item_id}"))?;
            let draft: TabTodoDraft = parse_source(from_pad.as_deref(), from_file.as_deref())?;
            client.call(
                "tab.todo.amend",
                Some(json!({
                    "tab_id": tab,
                    "caller_tab_id": actor,
                    "generation": checklist.generation,
                    "expected_revision": item.revision,
                    "item_id": item_id,
                    "item": draft,
                })),
            )
        }
        TabTodoAction::Reset { tab, from_file } => {
            require_user_context()?;
            let target = target_tab(tab)?;
            let current = optional_checklist(client, &target, None)?;
            let generation = current.as_ref().map_or(0, |checklist| checklist.generation);
            let items: Vec<TabTodoDraft> = parse_source(None, Some(&from_file))?;
            client.call(
                "tab.todo.reset",
                Some(json!({
                    "tab_id": target,
                    "generation": generation,
                    "items": items,
                })),
            )
        }
        TabTodoAction::Clear { item_id, all, tab } => {
            require_user_context()?;
            if !all && item_id.is_none() {
                return Err("tab todo clear requires an item id or --all".into());
            }
            let target = target_tab(tab)?;
            let checklist = required_checklist(client, &target, None)?;
            if all {
                client.call(
                    "tab.todo.clear_all",
                    Some(json!({
                        "tab_id": target,
                        "generation": checklist.generation,
                    })),
                )
            } else {
                let item_id = item_id.expect("validated clear item id");
                let item = checklist
                    .items
                    .iter()
                    .find(|item| item.id == item_id)
                    .ok_or_else(|| format!("checklist item not found: {item_id}"))?;
                client.call(
                    "tab.todo.clear_one",
                    Some(json!({
                        "tab_id": target,
                        "generation": checklist.generation,
                        "expected_revision": item.revision,
                        "item_id": item_id,
                    })),
                )
            }
        }
    }
}

fn list_agent_todos(client: &mut dyn RpcClient) -> Result<Value, String> {
    let tab_id = required_current_tab()?;
    client.call(
        "tab.todo.list",
        Some(json!({ "tab_id": tab_id, "caller_tab_id": tab_id })),
    )
}

fn write_agent_todos(client: &mut dyn RpcClient, from_pad: &str) -> Result<Value, String> {
    let tab_id = required_current_tab()?;
    let input: Vec<AgentTodoWriteItem> = parse_source(Some(from_pad), None)?;
    if input.len() > MAX_TAB_TODO_ITEMS {
        return Err(format!(
            "todo write accepts at most {MAX_TAB_TODO_ITEMS} items"
        ));
    }
    let in_progress = input
        .iter()
        .filter(|item| matches!(&item.status, AgentTodoStatus::InProgress))
        .count();
    if in_progress > 1 {
        return Err("todo write accepts at most one in_progress item".into());
    }
    let mut submitted_content = HashSet::new();
    for item in &input {
        if item.content.trim().is_empty()
            || item.content.chars().count() > MAX_TAB_TODO_TITLE_CHARS
            || item.content.chars().any(char::is_control)
        {
            return Err(format!(
                "todo content must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters without controls"
            ));
        }
        if item.active_form.trim().is_empty()
            || item.active_form.chars().count() > MAX_TAB_TODO_TITLE_CHARS
            || item.active_form.chars().any(char::is_control)
        {
            return Err(format!(
                "activeForm must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters without controls"
            ));
        }
        if !submitted_content.insert(item.content.clone()) {
            return Err(format!("todo write repeats content: {}", item.content));
        }
    }

    let mut checklist = required_checklist(client, &tab_id, Some(&tab_id))?;
    let mut changes = Vec::new();
    let mut matched_ids: HashSet<String> = HashSet::new();
    let mut final_in_progress = checklist
        .items
        .iter()
        .filter(|item| item.state == TabTodoState::InProgress)
        .count();
    for requested in input {
        let matches: Vec<_> = checklist
            .items
            .iter()
            .filter(|item| item.title == requested.content)
            .collect();
        if matches.len() != 1 {
            return Err(if matches.is_empty() {
                format!(
                    "todo write cannot add an unlocked item: {}",
                    requested.content
                )
            } else {
                format!("todo write content is ambiguous: {}", requested.content)
            });
        }
        let item = matches[0];
        if !matched_ids.insert(item.id.clone()) {
            return Err(format!("todo write repeats item: {}", item.id));
        }
        if requested
            .link
            .as_deref()
            .is_some_and(|link| !link_matches(item, link))
        {
            return Err(format!(
                "todo write cannot change the link for locked item {}",
                item.id
            ));
        }
        let next = match requested.status {
            AgentTodoStatus::Pending => TabTodoState::Open,
            AgentTodoStatus::InProgress => TabTodoState::InProgress,
            AgentTodoStatus::Completed => TabTodoState::Done,
        };
        if item.state != next {
            if item.state == TabTodoState::InProgress {
                final_in_progress = final_in_progress.saturating_sub(1);
            }
            if next == TabTodoState::InProgress {
                final_in_progress += 1;
            }
            changes.push((item.id.clone(), next));
        }
    }
    if final_in_progress > 1 {
        return Err("todo write would leave more than one in_progress item".into());
    }

    prioritize_state_changes(&mut changes);
    for (item_id, state) in changes {
        let item = checklist
            .items
            .iter()
            .find(|item| item.id == item_id)
            .ok_or_else(|| format!("checklist item disappeared during write: {item_id}"))?;
        let value = call_state(client, &tab_id, Some(&tab_id), &checklist, item, state)?;
        checklist = parse_checklist(value)?
            .ok_or_else(|| "tab.todo.state returned no checklist".to_string())?;
    }
    let omitted_locked_items = checklist.items.len().saturating_sub(matched_ids.len());
    Ok(json!({
        "checklist": checklist,
        "omitted_locked_items": omitted_locked_items,
        "message": format!("{omitted_locked_items} omitted locked item(s) remain unchanged"),
    }))
}

pub(super) fn prioritize_state_changes(changes: &mut [(String, TabTodoState)]) {
    changes.sort_by_key(|(_, state)| matches!(state, TabTodoState::InProgress));
}

fn call_state(
    client: &mut dyn RpcClient,
    target: &str,
    actor: Option<&str>,
    checklist: &TabTodoChecklist,
    item: &TabTodoItem,
    state: TabTodoState,
) -> Result<Value, String> {
    let state = match state {
        TabTodoState::Open => "open",
        TabTodoState::InProgress => "in_progress",
        TabTodoState::Done => "done",
    };
    let mut params = json!({
        "tab_id": target,
        "generation": checklist.generation,
        "expected_revision": item.revision,
        "item_id": item.id,
        "state": state,
    });
    if let Some(actor) = actor {
        params["caller_tab_id"] = json!(actor);
    }
    client.call("tab.todo.state", Some(params))
}

fn required_checklist(
    client: &mut dyn RpcClient,
    target: &str,
    actor: Option<&str>,
) -> Result<TabTodoChecklist, String> {
    optional_checklist(client, target, actor)?
        .ok_or_else(|| format!("tab {target} has no current checklist"))
}

fn optional_checklist(
    client: &mut dyn RpcClient,
    target: &str,
    actor: Option<&str>,
) -> Result<Option<TabTodoChecklist>, String> {
    let method = crate::cmd::tab::todo_list_rpc_method(target, actor);
    let mut params = json!({ "tab_id": target });
    if let Some(actor) = actor {
        params["caller_tab_id"] = json!(actor);
    }
    let value = client.call(method, Some(params))?;
    parse_checklist(value)
}

fn parse_checklist(value: Value) -> Result<Option<TabTodoChecklist>, String> {
    serde_json::from_value(value).map_err(|error| format!("invalid checklist response: {error}"))
}

fn target_tab(explicit: Option<String>) -> Result<String, String> {
    explicit
        .filter(|value| !value.trim().is_empty())
        .or_else(current_tab)
        .ok_or_else(|| "todo command requires --tab or CODETERM_TAB_ID".into())
}

fn current_tab() -> Option<String> {
    std::env::var("CODETERM_TAB_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn required_current_tab() -> Result<String, String> {
    current_tab().ok_or_else(|| "this todo operation requires CODETERM_TAB_ID".into())
}

fn require_user_context() -> Result<(), String> {
    if current_tab().is_some() {
        Err("user reset/clear commands must run outside an agent tab".into())
    } else {
        Ok(())
    }
}

fn parse_source<T: DeserializeOwned>(
    from_pad: Option<&str>,
    from_file: Option<&str>,
) -> Result<T, String> {
    let body = match (from_pad, from_file) {
        (Some(path), None) => crate::cmd::payload_source::read_from_pad(path)?,
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|error| format!("read todo input file {path}: {error}"))?,
        (None, None) => return Err("todo command requires --from-pad or --from-file".into()),
        (Some(_), Some(_)) => return Err("use only one of --from-pad or --from-file".into()),
    };
    if body.len() > MAX_TODO_INPUT_BYTES {
        return Err(format!("todo input exceeds {MAX_TODO_INPUT_BYTES} bytes"));
    }
    serde_json::from_str(&body).map_err(|error| format!("todo input is not valid JSON: {error}"))
}

fn link_matches(item: &TabTodoItem, requested: &str) -> bool {
    match item.link.as_ref() {
        Some(TabTodoLink::DomiosTask { task_id, label, .. }) => {
            requested == task_id || requested == label
        }
        Some(TabTodoLink::Url { url, label }) => requested == url || requested == label,
        None => false,
    }
}

fn print_value(value: &Value, json_output: bool) {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into())
        );
        return;
    }
    if let Some(checklist) = value.get("checklist") {
        if let Ok(checklist) = serde_json::from_value::<TabTodoChecklist>(checklist.clone()) {
            print_checklist(&checklist);
            if let Some(message) = value.get("message").and_then(Value::as_str) {
                println!("{message}");
            }
            return;
        }
    }
    match parse_checklist(value.clone()) {
        Ok(Some(checklist)) => print_checklist(&checklist),
        Ok(None) => println!("No current checklist."),
        Err(_) => println!("{}", value),
    }
}

fn print_checklist(checklist: &TabTodoChecklist) {
    println!(
        "Tab {} checklist generation {} revision {} ({:?})",
        checklist.tab_id, checklist.generation, checklist.revision, checklist.status
    );
    for item in &checklist.items {
        let state = match item.state {
            TabTodoState::Open => "pending",
            TabTodoState::InProgress => "in_progress",
            TabTodoState::Done => "completed",
        };
        println!("{}\t[{state}]\t{}", item.id, item.title);
        if let Some(link) = &item.link {
            println!("\t{}", render_link(link));
        }
    }
}

fn render_link(link: &TabTodoLink) -> String {
    match link {
        TabTodoLink::DomiosTask { task_id, label, .. } => format!("task: {label} ({task_id})"),
        TabTodoLink::Url { url, label } => format!("url: {label} ({url})"),
    }
}
