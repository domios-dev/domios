use std::io::Read;

use crate::args::plan::{
    PlanAction, PlanElementAction, PlanGroupsAction, PlanKindsAction, PlanLayoutAction,
    PlanLoopAction, PlanMilestoneAction, PlanNoteAction, PlanSectionAction, PlanTaskAction,
};
use crate::client::{SocketCallError, SocketClient};
use crate::cmd::print::print_object;
use serde_json::{json, Value};

fn resolve_text(value: Option<String>, file: Option<String>) -> Result<Option<String>, String> {
    if let Some(path) = file {
        return read_path_or_stdin(&path).map(Some);
    }
    match value {
        Some(v) if v == "-" => read_path_or_stdin("-").map(Some),
        other => Ok(other),
    }
}

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

fn owner_from_env() -> Value {
    let pane = std::env::var("CODETERM_PANE_ID").ok();
    json!({
        "pane_id": pane,
        "session_id": std::env::var("CODETERM_SESSION_ID").ok(),
        "title": std::env::var("CODETERM_PANE_TITLE").ok(),
    })
}

fn call(client: &mut SocketClient, method: &str, params: Value) -> Result<Value, String> {
    client
        .call_detailed(method, Some(params.clone()))
        .map_err(|e| render_plan_call_error(e, &params))
}

fn render_plan_call_error(err: SocketCallError, params: &Value) -> String {
    match err {
        SocketCallError::Rpc(rpc) => rpc
            .data
            .as_ref()
            .and_then(|data| render_plan_error(data, params))
            .unwrap_or_else(|| format!("RPC error {}: {}", rpc.code, rpc.message)),
        SocketCallError::Transport(message) => message,
    }
}

fn render_plan_error(data: &Value, params: &Value) -> Option<String> {
    let kind = data.get("type")?.as_str()?;
    let plan_id = params
        .get("plan_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<id>");
    let task_id = params.get("task_id").and_then(|v| v.as_str());
    let loop_id = params
        .get("loop_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<loop_id>");
    match kind {
        "undeclared_kind" => {
            let declared = data
                .get("declared")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let listed = if declared.is_empty() {
                "(none)".to_string()
            } else {
                declared.join("|")
            };
            let csv = if declared.is_empty() {
                "<new>".to_string()
            } else {
                format!("{},<new>", declared.join(","))
            };
            Some(format!(
                "✗ undeclared milestone kind.\nvalid kinds: {listed}\nfix: codeterm plan kinds set {plan_id} --kinds {csv}\nor declare and use it once with: codeterm plan milestone add {plan_id} {} --kind <new> --add-kind --text='status update'",
                task_id.unwrap_or("<task_id>")
            ))
        }
        "bad_status" => Some(format!(
            "✗ invalid task status.\nvalid statuses: pending|in_progress|blocked|done|cancelled\nfix: codeterm plan task update {plan_id} {} --status pending\nuse --status-label for free-text nuance.",
            task_id.unwrap_or("<task_id>")
        )),
        "not_found" => Some(format!(
            "✗ no plan '{plan_id}' was found.\nfix: codeterm plan list --active"
        )),
        "task_not_found" => Some(format!(
            "✗ no task '{}' was found in plan '{plan_id}'.\nfix: codeterm plan list --active",
            task_id.unwrap_or("<task_id>")
        )),
        "budget_exhausted" => {
            let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let budget = data.get("budget").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(format!(
                "✗ loop budget {count}/{budget} reached for loop '{loop_id}'; extend with codeterm plan loop set {loop_id} --budget N, or close the plan."
            ))
        }
        _ => None,
    }
}

fn render_plan_markdown(plan: &Value) -> String {
    let mut out = String::new();
    let title = plan.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    let id = plan.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let status = plan.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    out.push_str(&format!("# {title} (`{id}`)\n\n"));
    out.push_str(&format!("**Status:** {status}\n\n"));
    if let Some(sections) = plan.get("sections").and_then(|v| v.as_array()) {
        if !sections.is_empty() {
            out.push_str("## Sections\n\n");
            for s in sections {
                let st = s.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                out.push_str(&format!("### {st}\n\n"));
                if let Some(body) = s.get("body").and_then(|v| v.as_str()) {
                    if !body.is_empty() {
                        out.push_str(body);
                        out.push_str("\n\n");
                    }
                }
            }
        }
    }
    if let Some(tasks) = plan.get("tasks").and_then(|v| v.as_array()) {
        if !tasks.is_empty() {
            out.push_str("## Tasks\n\n");
            for t in tasks {
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let tid = t.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let st = t.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                out.push_str(&format!("- **{name}** (`{tid}`) — {st}\n"));
            }
            out.push('\n');
        }
    }
    out
}

fn render_coverage(val: &Value) -> String {
    let mut out = String::new();
    let frac = val
        .get("plan_covered_fraction")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    out.push_str(&format!("Coverage: {:.0}%\n", frac * 100.0));
    if let Some(sections) = val.get("sections").and_then(|v| v.as_array()) {
        for s in sections {
            let title = s.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let total = s.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            let status = s.get("status").and_then(|v| v.as_str()).unwrap_or("none");
            out.push_str(&format!("  - {title}: {total} task(s) [{status}]\n"));
        }
    }
    let gaps: Vec<&str> = val
        .get("gaps")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if gaps.is_empty() {
        out.push_str("Gaps: none — every section has at least one task.\n");
    } else {
        out.push_str(&format!("Gaps ({}): {}\n", gaps.len(), gaps.join(", ")));
    }
    out
}

pub fn handle_plan(client: &mut SocketClient, json: bool, action: PlanAction) {
    match run_plan(client, json, action) {
        Ok(None) => {}
        Ok(Some(val)) => print_plan_result(json, &val),
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    }
}

fn run_plan(
    client: &mut SocketClient,
    json: bool,
    action: PlanAction,
) -> Result<Option<Value>, String> {
    let result = match action {
        PlanAction::Create {
            title,
            spec,
            spec_file,
            kinds,
            project,
        } => {
            let spec_text = resolve_text(spec, spec_file)?;
            let mut params = json!({
                "title": title,
                "owner": owner_from_env(),
            });
            if let Some(k) = kinds {
                params["kinds"] = json!(k);
            }
            if let Some(p) = project {
                params["project_path"] = json!(p);
            }
            if let Some(s) = spec_text {
                params["spec"] = json!(s);
            }
            call(client, "plan.create", params)
        }

        PlanAction::List {
            project,
            workspace: _,
            active,
            archived,
            all,
            unowned,
            orchestrator,
        } => {
            let mut params = json!({});
            if let Some(p) = project {
                params["project"] = json!(p);
            }
            if unowned {
                params["unowned"] = json!(true);
            } else if active {
                params["status"] = json!("active");
            } else if archived {
                params["status"] = json!("archived");
            } else if !all {
                params["status"] = json!("active");
            }
            if let Some(o) = orchestrator {
                params["orchestrator"] = json!(o);
            }
            call(client, "plan.list", params)
        }

        PlanAction::Get { plan_id } => {
            let val = call(client, "plan.get", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            println!("{}", render_plan_markdown(&val));
            return Ok(None);
        }

        PlanAction::Coverage { plan_id } => {
            let val = call(client, "plan.coverage", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            println!("{}", render_coverage(&val));
            return Ok(None);
        }

        PlanAction::Groups { action } => match action {
            PlanGroupsAction::Set { plan_id, groups } => call(
                client,
                "plan.groups.set",
                json!({ "plan_id": plan_id, "groups": groups }),
            ),
        },

        PlanAction::Link { plan_id, to } => {
            call(client, "plan.link", json!({ "plan_id": plan_id, "to": to }))
        }

        PlanAction::Unlink { plan_id, to } => call(
            client,
            "plan.unlink",
            json!({ "plan_id": plan_id, "to": to }),
        ),

        PlanAction::Update {
            plan_id,
            title,
            spec: _,
            spec_file: _,
            status,
        } => {
            let mut params = json!({ "plan_id": plan_id });
            if let Some(t) = title {
                params["title"] = json!(t);
            }
            if let Some(s) = status {
                params["status"] = json!(s);
            }
            call(client, "plan.update", params)
        }

        PlanAction::Archive { plan_id } => {
            call(client, "plan.archive", json!({ "plan_id": plan_id }))
        }

        PlanAction::Delete { plan_id } => {
            call(client, "plan.delete", json!({ "plan_id": plan_id }))
        }

        PlanAction::Transfer {
            plan_id,
            to,
            reason,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "to": { "pane_id": to },
            });
            if let Some(r) = reason {
                params["reason"] = json!(r);
            }
            call(client, "plan.transfer", params)
        }

        PlanAction::Disown { plan_id } => {
            call(client, "plan.disown", json!({ "plan_id": plan_id }))
        }

        PlanAction::Claim { plan_id } => call(
            client,
            "plan.claim",
            json!({ "plan_id": plan_id, "owner": owner_from_env() }),
        ),

        PlanAction::Section { action } => match action {
            PlanSectionAction::Add {
                plan_id,
                title,
                body,
                body_file,
                parent,
            } => {
                let body_text = resolve_text(body, body_file)?.unwrap_or_default();
                let mut params = json!({
                    "plan_id": plan_id,
                    "title": title,
                    "body": body_text,
                });
                if let Some(p) = parent {
                    params["parent"] = json!(p);
                }
                call(client, "plan.section.add", params)
            }
            PlanSectionAction::Update {
                plan_id,
                section_id,
                title,
                body,
                body_file,
            } => {
                let body_text = resolve_text(body, body_file)?;
                let mut params = json!({ "plan_id": plan_id, "section_id": section_id });
                if let Some(t) = title {
                    params["title"] = json!(t);
                }
                if let Some(b) = body_text {
                    params["body"] = json!(b);
                }
                call(client, "plan.section.update", params)
            }
            PlanSectionAction::Rm {
                plan_id,
                section_id,
            } => call(
                client,
                "plan.section.rm",
                json!({ "plan_id": plan_id, "section_id": section_id }),
            ),
        },

        PlanAction::Kinds { action } => match action {
            PlanKindsAction::Set { plan_id, kinds } => call(
                client,
                "plan.kinds.set",
                json!({ "plan_id": plan_id, "kinds": kinds }),
            ),
        },

        PlanAction::Loop { action } => match action {
            PlanLoopAction::Create {
                plan_id,
                target,
                budget,
                stop_when,
            } => {
                let mut params = json!({
                    "plan_id": plan_id,
                    "target": target,
                    "budget": budget,
                });
                if let Some(s) = stop_when {
                    params["stop_when"] = json!(s);
                }
                call(client, "plan.loop.create", params)
            }
            PlanLoopAction::Set {
                loop_id,
                budget,
                stop_when,
            } => {
                let mut params = json!({ "loop_id": loop_id });
                if let Some(b) = budget {
                    params["budget"] = json!(b);
                }
                if let Some(s) = stop_when {
                    params["stop_when"] = json!(s);
                }
                call(client, "plan.loop.set", params)
            }
            PlanLoopAction::Status { loop_id } => {
                call(client, "plan.loop.status", json!({ "loop_id": loop_id }))
            }
            PlanLoopAction::Stop { loop_id } => {
                call(client, "plan.loop.stop", json!({ "loop_id": loop_id }))
            }
        },

        PlanAction::Task { action } => match action {
            PlanTaskAction::Add {
                plan_id,
                name,
                kind,
                sections,
                model,
                why,
                why_file,
                body,
                body_file,
                intent,
                intent_file,
                assign_pane,
                group,
                loop_from: _,
            } => {
                let why_text = resolve_text(why, why_file)?;
                let body_text = resolve_text(body, body_file)?;
                let intent_text = resolve_text(intent, intent_file)?;
                let mut params = json!({ "plan_id": plan_id, "name": name });
                if let Some(k) = kind {
                    params["type"] = json!(k);
                }
                if let Some(s) = sections {
                    params["sections"] = json!(s);
                }
                if let Some(m) = model {
                    params["model"] = json!(m);
                }
                if let Some(w) = why_text {
                    params["why"] = json!(w);
                }
                if let Some(b) = body_text {
                    params["body"] = json!(b);
                }
                if let Some(i) = intent_text {
                    params["intent"] = json!(i);
                }
                if let Some(p) = assign_pane {
                    params["assign_pane"] = json!(p);
                }
                if let Some(g) = group {
                    params["group"] = json!(g);
                }
                call(client, "plan.task.add", params)
            }
            PlanTaskAction::Update {
                plan_id,
                task_id,
                name,
                kind,
                model,
                why,
                why_file,
                status,
                status_label,
                body,
                body_file,
                intent,
                intent_file,
                group,
            } => {
                let why_text = resolve_text(why, why_file)?;
                let body_text = resolve_text(body, body_file)?;
                let intent_text = resolve_text(intent, intent_file)?;
                let mut params = json!({ "plan_id": plan_id, "task_id": task_id });
                if let Some(n) = name {
                    params["name"] = json!(n);
                }
                if let Some(k) = kind {
                    params["type"] = json!(k);
                }
                if let Some(m) = model {
                    params["model"] = json!(m);
                }
                if let Some(w) = why_text {
                    params["why"] = json!(w);
                }
                if let Some(s) = status {
                    params["status"] = json!(s);
                }
                if let Some(l) = status_label {
                    params["status_label"] = json!(l);
                }
                if let Some(b) = body_text {
                    params["body"] = json!(b);
                }
                if let Some(i) = intent_text {
                    params["intent"] = json!(i);
                }
                if let Some(g) = group {
                    params["group"] = json!(g);
                }
                call(client, "plan.task.update", params)
            }
            PlanTaskAction::Close {
                plan_id,
                task_id,
                result,
                result_file,
                mesh: _,
            } => {
                let result_text = resolve_text(result, result_file)?;
                let mut params = json!({ "plan_id": plan_id, "task_id": task_id });
                if let Some(r) = result_text {
                    params["result"] = json!(r);
                }
                call(client, "plan.task.close", params)
            }
            PlanTaskAction::Assign {
                plan_id,
                task_id,
                pane,
                mesh,
            } => {
                let mut params = json!({
                    "plan_id": plan_id,
                    "task_id": task_id,
                    "pane": pane,
                });
                if let Some(p) = mesh {
                    params["peer"] = json!(p);
                }
                call(client, "plan.task.assign", params)
            }
            PlanTaskAction::Rm { plan_id, task_id } => call(
                client,
                "plan.task.rm",
                json!({ "plan_id": plan_id, "task_id": task_id }),
            ),
        },

        PlanAction::Milestone { action } => match action {
            PlanMilestoneAction::Add {
                plan_id,
                task_id,
                kind,
                add_kind,
                text,
                text_file,
                from,
            } => {
                let text_val = resolve_text(text, text_file)?.unwrap_or_default();
                let mut params = json!({
                    "plan_id": plan_id,
                    "task_id": task_id,
                    "kind": kind,
                    "text": text_val,
                    "add_kind": add_kind,
                });
                if let Some(f) = from {
                    params["from"] = json!({ "pane_id": f });
                }
                call(client, "plan.milestone.add", params)
            }
            PlanMilestoneAction::Update {
                plan_id,
                task_id,
                milestone_id,
                kind,
                text,
                text_file,
            } => {
                let text_val = resolve_text(text, text_file)?;
                let mut params = json!({
                    "plan_id": plan_id,
                    "task_id": task_id,
                    "milestone_id": milestone_id,
                });
                if let Some(k) = kind {
                    params["kind"] = json!(k);
                }
                if let Some(t) = text_val {
                    params["text"] = json!(t);
                }
                call(client, "plan.milestone.update", params)
            }
            PlanMilestoneAction::Rm {
                plan_id,
                task_id,
                milestone_id,
            } => call(
                client,
                "plan.milestone.rm",
                json!({
                    "plan_id": plan_id,
                    "task_id": task_id,
                    "milestone_id": milestone_id,
                }),
            ),
        },

        PlanAction::Layout { action } => match action {
            PlanLayoutAction::Set {
                plan_id,
                element,
                x,
                y,
            } => {
                let mut params = json!({
                    "plan_id": plan_id,
                    "element": element,
                });
                if let Some(v) = x {
                    params["x"] = json!(v);
                }
                if let Some(v) = y {
                    params["y"] = json!(v);
                }
                call(client, "plan.layout.set", params)
            }
        },

        PlanAction::Note { action } => match action {
            PlanNoteAction::Add {
                plan_id,
                text,
                text_file,
                link_task,
                link_section,
            } => {
                let text_val = resolve_text(text, text_file)?.unwrap_or_default();
                let mut links = Vec::new();
                if let Some(t) = link_task {
                    links.push(t);
                }
                if let Some(s) = link_section {
                    links.push(s);
                }
                call(
                    client,
                    "plan.note.add",
                    json!({
                        "plan_id": plan_id,
                        "text": text_val,
                        "links": links,
                    }),
                )
            }
        },

        PlanAction::Element { action } => match action {
            PlanElementAction::Add {
                plan_id,
                kind,
                content,
                content_file,
                link,
            } => {
                let content_text = resolve_text(content, content_file)?;
                let mut params = json!({ "plan_id": plan_id, "kind": kind });
                if let Some(c) = content_text {
                    params["content"] = json!({ "text": c });
                }
                if let Some(l) = link {
                    params["links"] = json!(l.split(',').map(str::trim).collect::<Vec<_>>());
                }
                call(client, "plan.element.add", params)
            }
        },
    };

    result.map(Some)
}

fn print_plan_result(json: bool, val: &Value) {
    if json {
        println!("{}", serde_json::to_string_pretty(val).unwrap());
    } else if val.get("plan_id").is_some() && val.get("plan").is_none() {
        if let Some(id) = val.get("plan_id").and_then(|v| v.as_str()) {
            println!("{}", id);
        } else {
            print_object(val);
        }
    } else if val.get("task_id").is_some()
        || val.get("section_id").is_some()
        || val.get("loop_id").is_some()
        || val.get("milestone_id").is_some()
        || val.get("element_id").is_some()
    {
        let id = val
            .get("task_id")
            .or_else(|| val.get("section_id"))
            .or_else(|| val.get("loop_id"))
            .or_else(|| val.get("milestone_id"))
            .or_else(|| val.get("element_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!("{}", id);
    } else if val.get("plans").is_some() {
        if let Some(arr) = val.get("plans").and_then(|v| v.as_array()) {
            if arr.is_empty() {
                println!("(no plans)");
            } else {
                for item in arr {
                    let id = item.get("plan_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("{}\t{}", id, title);
                }
            }
        }
    } else if val.get("ok").is_some() {
        println!("OK");
    } else {
        print_object(val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_undeclared_kind_fix() {
        let msg = render_plan_error(
            &json!({ "type": "undeclared_kind", "declared": ["status_update", "review"] }),
            &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
        )
        .unwrap();
        assert!(msg.contains("✗ undeclared milestone kind"));
        assert!(msg.contains("valid kinds: status_update|review"));
        assert!(msg.contains("codeterm plan kinds set plan_1 --kinds status_update,review,<new>"));
        assert!(msg.contains("--add-kind"));
    }

    #[test]
    fn renders_bad_status_fix() {
        let msg = render_plan_error(
            &json!({ "type": "bad_status" }),
            &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
        )
        .unwrap();
        assert!(msg.contains("pending|in_progress|blocked|done|cancelled"));
        assert!(msg.contains("codeterm plan task update plan_1 task_1 --status pending"));
        assert!(msg.contains("--status-label"));
    }

    #[test]
    fn renders_unknown_plan_fix() {
        let msg = render_plan_error(
            &json!({ "type": "not_found" }),
            &json!({ "plan_id": "missing" }),
        )
        .unwrap();
        assert!(msg.contains("✗ no plan 'missing'"));
        assert!(msg.contains("codeterm plan list --active"));
    }

    #[test]
    fn renders_unknown_task_fix() {
        let msg = render_plan_error(
            &json!({ "type": "task_not_found" }),
            &json!({ "plan_id": "plan_1", "task_id": "missing_task" }),
        )
        .unwrap();
        assert!(msg.contains("✗ no task 'missing_task'"));
        assert!(msg.contains("codeterm plan list --active"));
    }

    #[test]
    fn renders_loop_budget_fix() {
        let msg = render_plan_error(
            &json!({ "type": "budget_exhausted", "count": 3, "budget": 3 }),
            &json!({ "loop_id": "loop_1" }),
        )
        .unwrap();
        assert!(msg.contains("✗ loop budget 3/3 reached"));
        assert!(msg.contains("codeterm plan loop set loop_1 --budget N"));
    }
}
