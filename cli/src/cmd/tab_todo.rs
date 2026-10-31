use codeterm_types::agent::{
    tab_todo_bound_refusal, validate_tab_todo_reason, TabTodoChecklist, TabTodoDraft, TabTodoItem,
    TabTodoLink, TabTodoRecitedItem, TabTodoState, TabTodoWriteEntry, MAX_TAB_TODO_ITEMS,
    MAX_TAB_TODO_TITLE_CHARS,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::args::tab_todo::TabTodoAction;
use crate::client::RpcClient;

const MAX_TODO_INPUT_BYTES: usize = 64 * 1024;
const WRITE_SCHEMA: &str = "Accepted schema: [{id?: string, content?: string, status: pending|in_progress|completed|blocked|skipped, activeForm?: string, link?: string, reason?: string}]. New items require content; activeForm defaults to content; blocked/skipped require reason.";

fn parse_write_items(value: Value) -> Result<Vec<AgentTodoWriteItem>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("todo input must be an array. {WRITE_SCHEMA}"))?;
    let mut errors = Vec::new();
    let mut parsed = Vec::new();
    for (index, value) in items.iter().enumerate() {
        let Some(fields) = value.as_object() else {
            errors.push(format!("item[{index}] must be an object"));
            continue;
        };
        let before = errors.len();
        for (field, value) in fields {
            if !matches!(
                field.as_str(),
                "id" | "content" | "status" | "activeForm" | "link" | "reason"
            ) {
                errors.push(format!("item[{index}].{field}: unknown field"));
            } else if !value.is_string()
                && !(value.is_null() && matches!(field.as_str(), "id" | "link" | "reason"))
            {
                errors.push(format!("item[{index}].{field}: expected a string"));
            }
        }
        if !fields
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| {
                matches!(
                    status,
                    "pending" | "in_progress" | "completed" | "blocked" | "skipped"
                )
            })
        {
            errors.push(format!(
                "item[{index}].status: expected pending|in_progress|completed|blocked|skipped"
            ));
        }
        if !fields.get("id").is_some_and(Value::is_string)
            && !fields
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| !content.trim().is_empty())
        {
            errors.push(format!("item[{index}].content: required for a new item"));
        }
        if fields
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "blocked" | "skipped"))
            && !fields
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| !reason.trim().is_empty())
        {
            errors.push(format!(
                "item[{index}].reason: blocked/skipped needs a reason"
            ));
        }
        if errors.len() == before {
            match serde_json::from_value::<AgentTodoWriteItem>(value.clone()) {
                Ok(item) => match write_entry(item.clone()) {
                    Ok(_) => parsed.push(item),
                    Err(error) => errors.push(format!("item[{index}]: {error}")),
                },
                Err(error) => errors.push(format!("item[{index}]: {error}")),
            }
        }
    }
    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(format!("{}\n{WRITE_SCHEMA}", errors.join("\n")))
    }
}

fn write_entries(input: Vec<AgentTodoWriteItem>) -> Result<Vec<TabTodoWriteEntry>, String> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (index, item) in input.into_iter().enumerate() {
        match write_entry(item) {
            Ok(entry) => entries.push(entry),
            Err(error) => errors.push(format!("item[{index}]: {error}")),
        }
    }
    if errors.is_empty() {
        Ok(entries)
    } else {
        Err(format!("{}\n{WRITE_SCHEMA}", errors.join("\n")))
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AgentTodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Skipped,
}

impl AgentTodoStatus {
    fn state(&self) -> TabTodoState {
        match self {
            Self::Pending => TabTodoState::Open,
            Self::InProgress => TabTodoState::InProgress,
            Self::Completed => TabTodoState::Done,
            Self::Blocked => TabTodoState::Blocked,
            Self::Skipped => TabTodoState::Skipped,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentTodoWriteItem {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    content: String,
    status: AgentTodoStatus,
    #[serde(default, rename = "activeForm")]
    active_form: String,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    reason: Option<String>,
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

pub fn handle_proposal(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: crate::args::todo::TodoAction,
) {
    let result = execute_proposal(client, action);
    match result {
        Ok(value) => print_value(&value, json_output),
        Err(error) => crate::cmd::print::exit_with_cli_error(
            json_output,
            &crate::cmd::print::CliError::new(error),
        ),
    }
}

fn execute_proposal(
    client: &mut dyn RpcClient,
    action: crate::args::todo::TodoAction,
) -> Result<Value, String> {
    use crate::args::todo::TodoAction;
    let actor = required_current_tab()?;
    let (method, tab, from_pad, note_from_pad) = match action {
        TodoAction::Submit => ("tab.todo.submit", actor.clone(), None, None),
        TodoAction::Approve { tab, note_from_pad } => {
            ("tab.todo.approve", tab, None, note_from_pad)
        }
        TodoAction::Amend {
            tab,
            from_pad,
            note_from_pad,
        } => ("tab.todo.review_amend", tab, Some(from_pad), note_from_pad),
        _ => return Err("expected checklist proposal command".into()),
    };
    let mut params = json!({ "tab_id": tab, "caller_tab_id": actor });
    if let Some(path) = from_pad {
        let input = parse_write_items(parse_source(Some(&path), None)?)?;
        let entries = write_entries(input)?;
        params["items"] = json!(entries);
    }
    if let Some(path) = note_from_pad {
        let note = crate::cmd::payload_source::read_from_pad(&path)?
            .trim()
            .to_owned();
        validate_tab_todo_reason(&note)?;
        params["note"] = json!(note);
    }
    client.call(method, Some(params))
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
            let value = call_state(
                client,
                &target,
                actor.as_deref(),
                &checklist,
                item,
                TabTodoState::Done,
                None,
            )?;
            let checklist = parse_checklist(value)?
                .ok_or_else(|| "tab.todo.state returned no checklist".to_string())?;
            Ok(with_remaining(checklist, None))
        }
        TabTodoAction::Reopen {
            item_id,
            tab,
            reason,
        } => {
            validate_tab_todo_reason(&reason)?;
            let mut params = json!({
                "tab_id": tab,
                "item_id": item_id,
                "reason": reason,
            });
            if let Some(actor) = current_tab() {
                params["caller_tab_id"] = json!(actor);
            }
            client.call("tab.todo.reopen", Some(params))
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
    let input = parse_write_items(parse_source(Some(from_pad), None)?)?;
    write_declared_todos(client, &tab_id, input)
}

/// The daemon creates the checklist on first write and reconciles it afterwards.
fn write_declared_todos(
    client: &mut dyn RpcClient,
    tab_id: &str,
    input: Vec<AgentTodoWriteItem>,
) -> Result<Value, String> {
    if input.len() > MAX_TAB_TODO_ITEMS {
        return Err(tab_todo_bound_refusal());
    }
    let entries = write_entries(input)?;
    let value = client.call(
        "tab.todo.write",
        Some(json!({
            "tab_id": tab_id,
            "caller_tab_id": tab_id,
            "items": entries,
        })),
    )?;
    let omitted_locked_items = value
        .get("omitted_locked_items")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let checklist = parse_checklist(value.get("checklist").cloned().unwrap_or(Value::Null))?
        .ok_or_else(|| "tab.todo.write returned no checklist".to_string())?;
    let tip = if checklist.proposal.is_none() {
        "; hint: share it with your parent using domios todo submit"
    } else {
        ""
    };
    let message = format!("{omitted_locked_items} omitted locked item(s) remain unchanged{tip}");
    let mut reply = with_remaining(checklist, Some(message));
    reply["omitted_locked_items"] = json!(omitted_locked_items);
    Ok(reply)
}

fn write_entry(item: AgentTodoWriteItem) -> Result<TabTodoWriteEntry, String> {
    let active_form = if item.active_form.is_empty() {
        &item.content
    } else {
        &item.active_form
    };
    if (item.id.is_none() && active_form.trim().is_empty())
        || active_form.chars().count() > MAX_TAB_TODO_TITLE_CHARS
        || active_form.chars().any(char::is_control)
    {
        return Err(format!(
            "activeForm must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters without controls"
        ));
    }
    let entry = TabTodoWriteEntry {
        id: item.id,
        content: item.content,
        state: item.status.state(),
        link: item.link,
        reason: item.reason,
    };
    entry.validate()?;
    Ok(entry)
}

/// The reply names what is left, so every completion shows the remaining steps.
fn with_remaining(checklist: TabTodoChecklist, message: Option<String>) -> Value {
    let remaining: Vec<TabTodoRecitedItem> = checklist.remaining();
    let mut reply = json!({
        "checklist": checklist,
        "remaining": remaining,
    });
    if let Some(message) = message {
        reply["message"] = json!(message);
    }
    reply
}

fn call_state(
    client: &mut dyn RpcClient,
    target: &str,
    actor: Option<&str>,
    checklist: &TabTodoChecklist,
    item: &TabTodoItem,
    state: TabTodoState,
    reason: Option<String>,
) -> Result<Value, String> {
    let mut params = json!({
        "tab_id": target,
        "generation": checklist.generation,
        "expected_revision": item.revision,
        "item_id": item.id,
        "state": state,
    });
    if let Some(reason) = reason {
        params["reason"] = json!(reason);
    }
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
    crate::nonempty_env("CODETERM_TAB_ID")
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
        (None, Some(path)) => crate::local_admission::read_local_text(path)
            .map_err(|error| format!("read todo input file {path}: {error}"))?,
        (None, None) => return Err("todo command requires --from-pad or --from-file".into()),
        (Some(_), Some(_)) => return Err("use only one of --from-pad or --from-file".into()),
    };
    if body.len() > MAX_TODO_INPUT_BYTES {
        return Err(format!("todo input exceeds {MAX_TODO_INPUT_BYTES} bytes"));
    }
    serde_json::from_str(&body).map_err(|error| format!("todo input is not valid JSON: {error}"))
}

fn print_value(value: &Value, json_output: bool) {
    if json_output {
        println!(
            "{}",
            crate::cmd::output::encode(value).unwrap_or_else(|_| "null".into())
        );
        return;
    }
    if !crate::cmd::output::verbose()
        && crate::cmd::output::is_collection()
        && crate::cmd::output::has_collection(value)
    {
        print!("{}", crate::cmd::output::compact_list(value, None));
        return;
    }
    if let Some(checklist) = value.get("checklist") {
        if let Ok(checklist) = serde_json::from_value::<TabTodoChecklist>(checklist.clone()) {
            print_checklist(&checklist);
            if let Some(message) = value.get("message").and_then(Value::as_str) {
                println!("{message}");
            }
            if let Some(remaining) = value.get("remaining").and_then(|remaining| {
                serde_json::from_value::<Vec<TabTodoRecitedItem>>(remaining.clone()).ok()
            }) {
                print_remaining(&remaining);
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
    if let Some(proposal) = &checklist.proposal {
        println!("Review: {:?}", proposal.state);
    }
    for item in &checklist.items {
        println!("{}\t[{}]\t{}", item.id, state_label(item.state), item.title);
        if let Some(reason) = &item.reason {
            println!("\treason: {reason}");
        }
        if item.attempt > 0 || item.counters.reopened > 0 {
            println!("\t{}", render_progress(item));
        }
        if let Some(link) = &item.link {
            println!("\t{}", render_link(link));
        }
    }
}

fn print_remaining(remaining: &[TabTodoRecitedItem]) {
    if remaining.is_empty() {
        println!("Remaining: none; every checklist item is completed or skipped.");
        return;
    }
    println!("Remaining ({}):", remaining.len());
    for item in remaining {
        println!("- [{}] {}", state_label(item.state), item.title);
    }
}

fn render_progress(item: &TabTodoItem) -> String {
    let counters = &item.counters;
    let mut text = format!(
        "attempt {} (started {}, completed {}, reopened {}, blocked {}, skipped {})",
        item.attempt,
        counters.started,
        counters.completed,
        counters.reopened,
        counters.blocked,
        counters.skipped
    );
    let durations: Vec<String> = item
        .attempt_spans()
        .iter()
        .filter_map(|span| {
            span.duration_secs
                .map(|secs| format!("#{} {secs}s", span.attempt))
        })
        .collect();
    if !durations.is_empty() {
        text.push_str(&format!("; durations {}", durations.join(", ")));
    }
    text
}

fn state_label(state: TabTodoState) -> &'static str {
    match state {
        TabTodoState::Open => "pending",
        TabTodoState::InProgress => "in_progress",
        TabTodoState::Done => "completed",
        TabTodoState::Blocked => "blocked",
        TabTodoState::Skipped => "skipped",
    }
}

fn render_link(link: &TabTodoLink) -> String {
    match link {
        TabTodoLink::DomiosTask { task_id, label, .. } => format!("task: {label} ({task_id})"),
        TabTodoLink::Url { url, label } => format!("url: {label} ({url})"),
    }
}

#[cfg(test)]
mod tests;
