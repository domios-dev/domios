use crate::args::task::{CanvasAction, TaskAction};
use crate::client::RpcClient;
use crate::cmd::hints::{extract_hints_from_value, print_hints_stderr};
use crate::cmd::plan::inject_trusted_session;
use crate::cmd::plan_output::canonical_mutation_id;
use crate::cmd::print::{exit_with_cli_error, print_object, CliError};
use serde_json::{json, Value};

fn call(client: &mut dyn RpcClient, method: &str, params: Value) -> Result<Value, String> {
    crate::cmd::plan::call_plan_rpc(client, method, params)
}

fn caller_tab(client: &dyn RpcClient) -> Option<String> {
    if client.is_remote() {
        return None;
    }
    std::env::var("CODETERM_TAB_ID")
        .ok()
        .filter(|s| !s.is_empty())
}

fn cwd_project(client: &dyn RpcClient) -> Option<String> {
    if client.is_remote() {
        return None;
    }
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
}

pub fn handle_task(client: &mut dyn RpcClient, json: bool, action: TaskAction) {
    let (val, show_opts) = run_task(client, json, action)
        .unwrap_or_else(|e| exit_with_cli_error(json, &CliError::new(e)));
    print_task_result(json, &val, show_opts);
}

pub fn handle_canvas(client: &mut dyn RpcClient, json: bool, action: CanvasAction) {
    let val =
        run_canvas(client, action).unwrap_or_else(|e| exit_with_cli_error(json, &CliError::new(e)));
    print_task_result(json, &val, None);
}

fn run_canvas(client: &mut dyn RpcClient, action: CanvasAction) -> Result<Value, String> {
    match action {
        CanvasAction::Focus { task, tab, plan } => {
            let mut params = json!({});
            if let Some(id) = task {
                params["task_id"] = json!(id);
            }
            if let Some(id) = tab {
                params["tab_id"] = json!(id);
            }
            if let Some(id) = plan {
                params["plan_id"] = json!(id);
            }
            call(client, "canvas.focus", params)
        }
        CanvasAction::Filter { facet } => call(client, "canvas.filter", json!({ "facet": facet })),
    }
}

fn run_task(
    client: &mut dyn RpcClient,
    _json_output: bool,
    action: TaskAction,
) -> Result<(Value, Option<TaskShowOpts>), String> {
    match action {
        TaskAction::List {
            active,
            all,
            mine,
            unassigned,
            workspace: _,
            plan,
            status,
        } => {
            let mut params = json!({
                "active": status.is_none() && (active || !all),
                "mine": mine,
                "unassigned": unassigned,
            });
            if let Some(project) = cwd_project(client) {
                params["project"] = json!(project);
            }
            if let Some(caller) = caller_tab(client) {
                params["caller_tab_id"] = json!(caller);
            }
            if let Some(plan_id) = plan {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(status) = status {
                params["status"] = json!(status);
            }
            call(client, "task.list", params).map(|val| (val, None))
        }
        TaskAction::Show {
            task_id,
            siblings,
            all,
        } => call(client, "task.show", json!({ "task_id": task_id })).map(|val| {
            (
                val,
                Some(TaskShowOpts {
                    list_siblings: siblings,
                    include_done_siblings: all,
                }),
            )
        }),
        TaskAction::Take { task_id } => {
            let caller = caller_tab(client).ok_or(
                "task take requires a Tab on the target machine and is unavailable over mesh",
            )?;
            let mut params = json!({ "task_id": task_id, "caller_tab_id": caller });
            inject_trusted_session(&mut params);
            call(client, "task.take", params).map(|val| (val, None))
        }
        TaskAction::Drop { task_id } => {
            let mut params = json!({ "task_id": task_id });
            inject_trusted_session(&mut params);
            call(client, "task.drop", params).map(|val| (val, None))
        }
    }
}

pub(crate) struct TaskShowOpts {
    pub list_siblings: bool,
    pub include_done_siblings: bool,
}

fn empty_task_list_message(val: &Value) -> String {
    val.get("empty_reason")
        .and_then(crate::cmd::plan::render_empty_reason)
        .unwrap_or_else(|| "(no tasks)".into())
}

pub(crate) fn print_task_result(json: bool, val: &Value, show_opts: Option<TaskShowOpts>) {
    if json {
        println!("{}", serde_json::to_string_pretty(val).unwrap());
        return;
    }

    let hints = extract_hints_from_value(val);

    if let Some(tasks) = val.get("tasks").and_then(|v| v.as_array()) {
        if tasks.is_empty() {
            println!("{}", empty_task_list_message(val));
            return;
        }
        print!(
            "{}",
            crate::cmd::task_list_render::render_simple_task_list(tasks)
        );
        return;
    } else if let Some(task) = val.get("task") {
        let id = task.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let name = task.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let plan = val.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
        let plan_title = val
            .get("plan_title")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!("{id}\t{status}\t{plan}\t{name}");
        println!("key: {}", crate::cmd::task_list_render::task_key_label(val));
        println!("plan: {plan_title} ({plan})");
        print_task_presentation(val);
        if let Some(reason) = blocked_reason_line(task) {
            println!("{reason}");
        }
        print_competition(task, plan, id);
        print_executor_role(task);
        print_executor(task);
        print_task_routing(task);
        print_external_refs(task);
        if let Some(body) = task
            .get("body")
            .and_then(|v| v.as_str())
            .filter(|b| !b.trim().is_empty())
        {
            println!("---");
            println!("{}", body.trim_end());
            println!("---");
        }
        print_task_quality(task);
        print_strides(task);
        print_attachments(task.get("attachments"));
        print_label_list("sections", val.get("sections"));
        print_single_label("parent", val.get("parent"));
        let opts = show_opts.unwrap_or(TaskShowOpts {
            list_siblings: false,
            include_done_siblings: false,
        });
        print_siblings(val.get("siblings"), &opts);
        print_label_list("subtasks", val.get("subtasks"));
        println!("record: domios plan record {plan} --task {id} --step status_update:<text>");
    } else if canonical_mutation_id(val).is_some() {
        print_hints_stderr(&hints);
        if let Some(id) = canonical_mutation_id(val) {
            println!("{id}");
        }
        return;
    } else if val.get("ok").is_some() {
        println!("OK");
    } else {
        print_object(val);
    }
    print_hints_stderr(&hints);
}

fn print_task_routing(task: &Value) {
    for line in task_routing_lines(task) {
        println!("{line}");
    }
}

pub(crate) fn task_routing_lines(task: &Value) -> Vec<String> {
    [
        ("role_guidance", "role_guidance"),
        ("personality", "personality"),
        ("model", "model"),
    ]
    .into_iter()
    .map(|(label, field)| {
        let value = task
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(unset)");
        format!("{label}: {value}")
    })
    .collect()
}

fn print_task_presentation(val: &Value) {
    if let Some(line) = task_presentation_line(val) {
        println!("{line}");
    }
}

fn blocked_reason_line(task: &Value) -> Option<String> {
    if task.get("status").and_then(Value::as_str) != Some("blocked") {
        return None;
    }
    task.get("status_label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(|reason| format!("blocked reason: {reason}"))
}

fn task_presentation_line(val: &Value) -> Option<String> {
    match val.get("presentation").and_then(|value| value.as_str()) {
        Some("actionable_pending") => Some("presentation: actionable pending".into()),
        Some("handoff_required") => {
            Some("presentation: handoff required (sealed checkpoint awaits adoption)".into())
        }
        Some("blocked_on_subtasks") => Some("presentation: blocked on subtasks".into()),
        Some("parked_for_later_group") => Some("presentation: parked for later groups".into()),
        Some("waiting_on_dependencies") => {
            let dependencies = val
                .get("dependency_waits")
                .and_then(|value| value.as_array())
                .map(|waits| {
                    waits
                        .iter()
                        .map(|wait| {
                            let id = wait
                                .get("task_id")
                                .and_then(|value| value.as_str())
                                .unwrap_or("?");
                            let name = wait
                                .get("name")
                                .and_then(|value| value.as_str())
                                .unwrap_or("dependency");
                            format!("{id} ({name})")
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            Some(format!(
                "presentation: waiting on dependency — {dependencies}"
            ))
        }
        Some("paused") => Some("presentation: paused (explicit operator hold)".into()),
        _ => None,
    }
}

fn is_closed_sibling_status(status: &str) -> bool {
    matches!(status, "done" | "cancelled")
}

pub(crate) fn short_display_id(id: &str) -> String {
    if let Some((prefix, slug)) = id.split_once('_') {
        let cut: String = slug.chars().take(8).collect();
        format!("{prefix}_{cut}")
    } else {
        id.chars().take(12).collect()
    }
}

fn truncate_label(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

fn print_siblings(value: Option<&Value>, opts: &TaskShowOpts) {
    let items = value
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        println!("siblings: none");
        return;
    }
    if opts.list_siblings {
        let rendered = items
            .iter()
            .filter(|item| {
                opts.include_done_siblings
                    || !item
                        .get("status")
                        .and_then(|v| v.as_str())
                        .is_some_and(is_closed_sibling_status)
            })
            .map(|item| {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = item
                    .get("name")
                    .or_else(|| item.get("title"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                format!(
                    "{} {} ({status})",
                    short_display_id(id),
                    truncate_label(name, 48)
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        if rendered.is_empty() {
            println!("siblings: none");
        } else {
            println!("siblings: {rendered}");
        }
        return;
    }
    let open = items
        .iter()
        .filter(|item| {
            !item
                .get("status")
                .and_then(|v| v.as_str())
                .is_some_and(is_closed_sibling_status)
        })
        .count();
    let closed = items.len().saturating_sub(open);
    if closed > 0 {
        println!("siblings: {open} open (+ {closed} done/cancelled); use --siblings to list");
    } else {
        println!("siblings: {open} open; use --siblings to list");
    }
}

fn print_attachments(value: Option<&Value>) {
    let items = value
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        println!("attachments: none");
        return;
    }
    println!("attachments:");
    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("attachment");
        let path = item.get("path").and_then(|v| v.as_str()).unwrap_or("?");
        let media = item
            .get("media_type")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let size = item.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        println!("- {name} ({media}, {size} bytes): {path}");
    }
}

fn print_competition(task: &Value, plan: &str, task_id: &str) {
    let Some(competition) = task.get("competition").filter(|v| !v.is_null()) else {
        return;
    };
    if competition.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return;
    }
    let attempts = competition
        .get("attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(2)
        .max(2);
    let mut parts = vec![format!("{attempts} attempts")];
    let models = competition
        .get("models")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    if !models.is_empty() {
        parts.push(format!("models {models}"));
    }
    if let Some(hint) = competition
        .get("judge_hint")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("judge: {hint}"));
    }
    println!("competition: {}", parts.join("; "));
    println!("compete: plan.task.compete route for {plan} {task_id}");
}

fn executor_role_line(task: &Value) -> Option<String> {
    task.get("executor_role")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|role| format!("executor_role: {role}"))
}

fn print_executor_role(task: &Value) {
    if let Some(line) = executor_role_line(task) {
        println!("{line}");
    }
}

/// The tab actually executing this task, and the mesh peer it runs on. Recorded
/// structurally by `plan task assign --tab <id> [--mesh <peer>]`; rendered here
/// so a mesh-staffed task stops carrying its executor in a free-text reason,
/// which is a record with no reader.
fn executor_line(task: &Value) -> Option<String> {
    let field = |key: &str| {
        task.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let tab = field("assigned_tab_id").or_else(|| {
        task.get("lane")
            .and_then(|lane| lane.get("tab_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })?;
    let mut line = format!("executor: tab {tab}");
    if let Some(peer) = field("peer") {
        line.push_str(&format!(" on peer {peer}"));
    }
    if let Some(role) = field("taken_by_role") {
        line.push_str(&format!(" (role {role})"));
    }
    Some(line)
}

fn print_executor(task: &Value) {
    if let Some(line) = executor_line(task) {
        println!("{line}");
    }
}

fn task_quality_lines(task: &Value) -> Vec<String> {
    let mut lines = vec![];
    if let Some(items) = task.get("acceptance_criteria").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            let rendered = items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("criteria: {rendered}"));
        }
    }
    for (label, key) in [
        ("test_plan", "test_plan"),
        ("qa_plan", "qa_plan"),
        ("correctness", "correctness"),
    ] {
        if let Some(value) = task.get(key).and_then(|v| v.as_str()) {
            if !value.is_empty() {
                lines.push(format!("{label}: {value}"));
            }
        }
    }
    lines
}

fn print_task_quality(task: &Value) {
    for line in task_quality_lines(task) {
        println!("{line}");
    }
}

fn print_external_refs(task: &Value) {
    for line in render_external_refs(task) {
        println!("{line}");
    }
}

fn render_external_refs(task: &Value) -> Vec<String> {
    task.get("external_refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("canonical_url").and_then(Value::as_str)?;
            let relationship = item
                .get("relationship")
                .and_then(Value::as_str)
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            Some(format!("external_ref: {url}{relationship}"))
        })
        .collect()
}

fn milestone_author(from: Option<&Value>) -> Option<String> {
    let from = from?;
    let title = from
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(title) = title {
        return Some(title.to_string());
    }
    from.get("tab_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(short_display_id)
}

fn format_milestone_ts(ts: u64) -> String {
    if ts == 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp_millis(ts as i64)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn render_stride_line(milestone: &Value) -> String {
    let ts = milestone.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
    let kind = milestone
        .get("kind")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("?");
    let mut line = format!("- {} [{kind}]", format_milestone_ts(ts));
    if let Some(author) = milestone_author(milestone.get("from")) {
        line.push_str(&format!(" {author}"));
    }
    let text = milestone
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or_default();
    if !text.is_empty() {
        line.push_str(&format!(": {text}"));
    }
    line
}

fn render_strides(task: &Value) -> Option<String> {
    let items = task.get("milestones").and_then(|v| v.as_array())?;
    if items.is_empty() {
        return None;
    }
    let mut sorted: Vec<&Value> = items.iter().collect();
    sorted.sort_by(|a, b| {
        let ta = a.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
        let tb = b.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
        tb.cmp(&ta)
    });
    let mut out = String::from("strides:");
    for milestone in sorted {
        out.push('\n');
        out.push_str(&render_stride_line(milestone));
    }
    Some(out)
}

fn print_strides(task: &Value) {
    if let Some(block) = render_strides(task) {
        println!("{block}");
    }
}

fn print_label_list(label: &str, value: Option<&Value>) {
    println!("{}", label_list_line(label, value));
}

fn label_list_line(label: &str, value: Option<&Value>) -> String {
    let items = value
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return format!("{label}: none");
    }
    let rendered = items
        .iter()
        .map(|item| {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let name = item
                .get("name")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            match item.get("status").and_then(|v| v.as_str()) {
                Some(status) => format!("{name} ({id}, {status})"),
                None => format!("{name} ({id})"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("{label}: {rendered}")
}

fn print_single_label(label: &str, value: Option<&Value>) {
    let Some(item) = value.filter(|v| !v.is_null()) else {
        println!("{label}: none");
        return;
    };
    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let name = item
        .get("name")
        .or_else(|| item.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    match item.get("status").and_then(|v| v.as_str()) {
        Some(status) => println!("{label}: {name} ({id}, {status})"),
        None => println!("{label}: {name} ({id})"),
    }
}

#[cfg(test)]
mod tests;
