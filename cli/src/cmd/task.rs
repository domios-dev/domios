use std::io::Read;

use crate::args::task::{CanvasAction, TaskAction};
use crate::client::SocketClient;
use crate::cmd::hints::{
    extract_hints_from_value, maybe_orchestration_reminder, print_hints_stderr, push_hints,
    task_model_hint, task_quality_guard_hint, task_sections_hint,
};
use crate::cmd::plan_errors::render_plan_call_error;
use crate::cmd::plan_output::canonical_mutation_id;
use crate::cmd::print::print_object;
use serde_json::{json, Value};

fn read_path_or_stdin(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read stdin: {}", e))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {}: {}", path, e))
    }
}

fn resolve_text(value: Option<String>, file: Option<String>) -> Result<Option<String>, String> {
    if let Some(path) = file {
        return read_path_or_stdin(&path).map(Some);
    }
    match value {
        Some(v) if v == "-" => read_path_or_stdin("-").map(Some),
        other => Ok(other),
    }
}

fn call(client: &mut SocketClient, method: &str, params: Value) -> Result<Value, String> {
    client
        .call_detailed(method, Some(params.clone()))
        .map_err(|err| render_plan_call_error(err, &params))
}

fn caller_pane() -> Option<String> {
    std::env::var("CODETERM_PANE_ID")
        .ok()
        .filter(|s| !s.is_empty())
}

fn cwd_project() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
}

pub fn handle_task(client: &mut SocketClient, json: bool, action: TaskAction) {
    match run_task(client, json, action) {
        Ok((val, show_opts)) => print_task_result(json, &val, show_opts),
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    }
}

pub fn handle_canvas(client: &mut SocketClient, json: bool, action: CanvasAction) {
    let result = match action {
        CanvasAction::Focus { task, pane, plan } => {
            let mut params = json!({});
            if let Some(id) = task {
                params["task_id"] = json!(id);
            }
            if let Some(id) = pane {
                params["pane_id"] = json!(id);
            }
            if let Some(id) = plan {
                params["plan_id"] = json!(id);
            }
            call(client, "canvas.focus", params)
        }
        CanvasAction::Filter { facet } => call(client, "canvas.filter", json!({ "facet": facet })),
    };
    match result {
        Ok(val) => print_task_result(json, &val, None),
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    }
}

fn run_task(
    client: &mut SocketClient,
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
            if let Some(project) = cwd_project() {
                params["project"] = json!(project);
            }
            if let Some(caller) = caller_pane() {
                params["caller_pane_id"] = json!(caller);
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
            let caller = caller_pane().ok_or("CODETERM_PANE_ID is required for task take")?;
            call(
                client,
                "task.take",
                json!({ "task_id": task_id, "caller_pane_id": caller }),
            )
            .map(|val| (val, None))
        }
        TaskAction::Drop { task_id } => {
            call(client, "task.drop", json!({ "task_id": task_id })).map(|val| (val, None))
        }
        TaskAction::Add {
            name,
            kind,
            model,
            why,
            why_file,
            criterion,
            test_plan,
            correctness,
            body,
            body_file,
            intent,
            intent_file,
            assign_pane,
        } => {
            let has_model = model.as_deref().is_some_and(|s| !s.trim().is_empty());
            let why_text = resolve_text(why, why_file)?;
            let has_why = why_text.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_criteria = criterion.iter().any(|s| !s.trim().is_empty());
            let has_test_plan = test_plan.as_deref().is_some_and(|s| !s.trim().is_empty());
            let has_correctness = correctness.as_deref().is_some_and(|s| !s.trim().is_empty());
            let mut params = json!({
                "name": name,
                "type": kind.unwrap_or_else(|| "task".to_string()),
            });
            if let Some(project) = cwd_project() {
                params["project"] = json!(project);
            }
            if let Some(caller) = caller_pane() {
                params["caller_pane_id"] = json!(caller);
                params["owner"] = json!({ "pane_id": caller });
            }
            if let Some(m) = model {
                params["model"] = json!(m);
            }
            if let Some(w) = why_text {
                params["why"] = json!(w);
            }
            if !criterion.is_empty() {
                params["acceptance_criteria"] = json!(criterion);
            }
            if let Some(test_plan) = test_plan {
                params["test_plan"] = json!(test_plan);
            }
            if let Some(correctness) = correctness {
                params["correctness"] = json!(correctness);
            }
            if let Some(b) = resolve_text(body, body_file)? {
                params["body"] = json!(b);
            }
            if let Some(i) = resolve_text(intent, intent_file)? {
                params["intent"] = json!(i);
            }
            if let Some(pane) = assign_pane {
                params["assign_pane"] = json!(pane);
            }
            let mut val = call(client, "task.add", params)?;
            if let (Some(plan_id), Some(task_id)) = (
                val.get("plan_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                val.get("task_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            ) {
                let mut hints = vec![task_sections_hint(&plan_id, &task_id)];
                if !has_model || !has_why {
                    hints.push(task_model_hint(&plan_id, &task_id));
                }
                if let Some(hint) = task_quality_guard_hint(
                    &plan_id,
                    &task_id,
                    has_criteria,
                    has_test_plan,
                    has_correctness,
                ) {
                    hints.push(hint);
                }
                if let Some(hint) = maybe_orchestration_reminder() {
                    hints.push(hint);
                }
                push_hints(&mut val, hints);
            }
            Ok((val, None))
        }
    }
}

pub(crate) struct TaskShowOpts {
    pub list_siblings: bool,
    pub include_done_siblings: bool,
}

fn print_task_result(json: bool, val: &Value, show_opts: Option<TaskShowOpts>) {
    if json {
        println!("{}", serde_json::to_string_pretty(val).unwrap());
        return;
    }

    let hints = extract_hints_from_value(val);

    if let Some(tasks) = val.get("tasks").and_then(|v| v.as_array()) {
        if tasks.is_empty() {
            println!("(no tasks)");
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
        println!("plan: {plan_title} ({plan})");
        print_competition(task, plan, id);
        print_executor_role(task);
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
        print_attachments(task.get("attachments"));
        print_label_list("sections", val.get("sections"));
        print_single_label("parent", val.get("parent"));
        let opts = show_opts.unwrap_or(TaskShowOpts {
            list_siblings: false,
            include_done_siblings: false,
        });
        print_siblings(val.get("siblings"), &opts);
        print_label_list("subtasks", val.get("subtasks"));
        println!("record: codeterm plan record {plan} --task {id} --step status_update:<text>");
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
    println!("compete: plan.task.compete RPC/API route for {plan} {task_id}");
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

fn print_task_quality(task: &Value) {
    if let Some(items) = task.get("acceptance_criteria").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            let rendered = items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            println!("criteria: {rendered}");
        }
    }
    if let Some(test_plan) = task.get("test_plan").and_then(|v| v.as_str()) {
        if !test_plan.is_empty() {
            println!("test_plan: {test_plan}");
        }
    }
    if let Some(correctness) = task.get("correctness").and_then(|v| v.as_str()) {
        if !correctness.is_empty() {
            println!("correctness: {correctness}");
        }
    }
}

fn print_label_list(label: &str, value: Option<&Value>) {
    let items = value
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        println!("{label}: none");
        return;
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
    println!("{label}: {rendered}");
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
mod siblings_render_tests {
    use super::{is_closed_sibling_status, short_display_id, truncate_label, TaskShowOpts};
    use serde_json::json;

    fn render_siblings_line(items: &[serde_json::Value], opts: &TaskShowOpts) -> String {
        if !opts.list_siblings {
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
            return if closed > 0 {
                format!("siblings: {open} open (+ {closed} done/cancelled); use --siblings to list")
            } else {
                format!("siblings: {open} open; use --siblings to list")
            };
        }
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
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?");
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
            "siblings: none".into()
        } else {
            format!("siblings: {rendered}")
        }
    }

    #[test]
    fn default_siblings_is_one_summary_line() {
        let siblings: Vec<_> = (0..160)
            .map(|i| {
                json!({
                    "id": format!("task_{i:03}"),
                    "name": format!("Sibling {i}"),
                    "status": if i % 3 == 0 { "done" } else { "pending" }
                })
            })
            .collect();
        let line = render_siblings_line(
            &siblings,
            &TaskShowOpts {
                list_siblings: false,
                include_done_siblings: false,
            },
        );
        assert!(!line.contains("Sibling 0"));
        assert!(line.contains("use --siblings to list"));
    }

    #[test]
    fn siblings_flag_uses_short_ids_and_truncated_names() {
        let line = render_siblings_line(
            &[json!({
                "id": "task_abcdef012345",
                "name": "A very long sibling name that should be truncated for terminal display",
                "status": "pending"
            })],
            &TaskShowOpts {
                list_siblings: true,
                include_done_siblings: false,
            },
        );
        assert!(line.contains("task_abcdef"));
        assert!(line.contains('…'));
    }
}

#[cfg(test)]
mod executor_role_render_tests {
    use super::executor_role_line;
    use serde_json::json;

    #[test]
    fn renders_line_when_set() {
        let task = json!({ "id": "task_1", "executor_role": "qa" });
        assert_eq!(
            executor_role_line(&task).as_deref(),
            Some("executor_role: qa")
        );
    }

    #[test]
    fn omits_line_when_absent_or_blank() {
        assert!(executor_role_line(&json!({ "id": "task_1" })).is_none());
        assert!(executor_role_line(&json!({ "executor_role": "  " })).is_none());
        assert!(executor_role_line(&json!({ "executor_role": null })).is_none());
    }
}
