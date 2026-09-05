use std::io::Read;

use crate::args::plan::{
    PlanAction, PlanElementAction, PlanFacetsAction, PlanGoalAction, PlanGroupsAction,
    PlanKindsAction, PlanLayoutAction, PlanLoopAction, PlanMilestoneAction, PlanNoteAction,
};
use crate::client::{RpcClient, SocketCallError};
use crate::cmd::hints::{plan_create_spec_first_hint, push_hints};
use crate::cmd::plan_errors::render_plan_call_error;
use crate::cmd::print::CliError;

mod adopt_search;
pub(crate) mod caller_context;
mod gate_cmd;
mod qa_cmd;
mod read_cmd;
mod review_cmd;
mod task_cmd;
use self::adopt_search::{render_adopt_report, render_plan_search, resolve_source_plan};
use self::caller_context::CallerContext;
#[cfg(test)]
use crate::cmd::plan_errors::render_plan_error;
use crate::cmd::plan_markdown::{render_goal_block, render_plan_markdown};
use crate::cmd::plan_render::{print_plan_result, render_coverage, render_plan_sections};
use serde_json::{json, Value};
pub(crate) use task_cmd::render_empty_reason;

pub(crate) fn resolve_text(
    value: Option<String>,
    file: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(path) = file {
        return read_path_or_stdin(&path).map(Some);
    }
    match value {
        Some(v) if v == "-" => read_path_or_stdin("-").map(Some),
        other => Ok(other),
    }
}

pub(crate) fn resolve_role_guidance(
    value: Option<String>,
    file: Option<String>,
) -> Result<Option<String>, String> {
    let guidance = resolve_text(value, file)?;
    if guidance
        .as_deref()
        .is_some_and(|text| text.trim().is_empty())
    {
        return Err("--role-guidance conditions the worker and cannot be blank".into());
    }
    Ok(guidance)
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

pub(crate) fn owner_from_env() -> Value {
    CallerContext::from_env().owner()
}

pub(crate) fn caller_tab_id() -> Option<String> {
    CallerContext::from_env().tab_id()
}

pub(crate) fn maybe_inject_caller_tab(params: &mut Value) {
    CallerContext::from_env().apply_caller_tab(params);
}

pub(crate) fn inject_trusted_session(params: &mut Value) {
    CallerContext::from_env().apply_trusted_session(params);
}

/// The rendered explanation an operator sees, plus the daemon's code, which
/// `--json` needs and the rendered prose deliberately never carries.
fn plan_call_cli_error(error: SocketCallError, params: &Value) -> CliError {
    let code = match &error {
        SocketCallError::Rpc(rpc) => Some(rpc.code),
        SocketCallError::Transport(_) => None,
    };
    CliError {
        message: render_plan_call_error(error, params),
        code,
    }
}

pub(crate) fn call_plan_rpc(
    client: &mut dyn RpcClient,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    call_plan_rpc_detailed(client, method, params).map_err(|error| error.message)
}

pub(crate) fn call_plan_rpc_detailed(
    client: &mut dyn RpcClient,
    method: &str,
    mut params: Value,
) -> Result<Value, CliError> {
    if method == "plan.task.adopt_checkpoint" {
        inject_trusted_session(&mut params);
    } else if params.get("task_id").is_some()
        && (method.starts_with("plan.task.") || matches!(method, "task.take" | "task.drop"))
        && !matches!(
            method,
            "plan.task.add"
                | "plan.task.attempts"
                | "plan.task.attempt.show"
                | "plan.task.adopt_checkpoint"
        )
    {
        inject_trusted_session(&mut params);
        let lookup = params.clone();
        let shown = client
            .call_detailed("task.show", Some(lookup))
            .map_err(|error| plan_call_cli_error(error, &params))?;
        let owner_epoch = shown
            .get("task")
            .unwrap_or(&shown)
            .get("interruption")
            .and_then(|value| value.get("owner_epoch"))
            .and_then(Value::as_u64)
            .ok_or_else(|| CliError::new("task mutation could not resolve owner_epoch"))?;
        if let Some(object) = params.as_object_mut() {
            object.insert("owner_epoch".into(), json!(owner_epoch));
        }
    }
    client
        .call_detailed(method, Some(params.clone()))
        .map_err(|e| plan_call_cli_error(e, &params))
}

fn call(client: &mut dyn RpcClient, method: &str, params: Value) -> Result<Value, String> {
    call_plan_rpc(client, method, params)
}

pub(crate) fn validate_plan_delete_confirmation(plan_id: &str, yes: bool) -> Result<(), String> {
    if !yes {
        return Err(format!(
            "refusing to delete plan '{plan_id}' without --yes; pass --yes or --confirm to permanently delete it"
        ));
    }
    Ok(())
}

fn plan_delete_params(plan_id: String, yes: bool) -> Result<Value, String> {
    validate_plan_delete_confirmation(&plan_id, yes)?;
    Ok(json!({ "plan_id": plan_id }))
}

fn plan_transfer_params(
    plan_id: String,
    to: String,
    reason: Option<String>,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    let caller_tab_id = caller_tab_id
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| {
            "plan transfer requires CODETERM_TAB_ID from the calling Team manager".to_string()
        })?;
    let mut params = json!({
        "plan_id": plan_id,
        "to": { "tab_id": to },
        "caller_tab_id": caller_tab_id,
    });
    if let Some(reason) = reason {
        params["reason"] = json!(reason);
    }
    Ok(params)
}

fn default_project_path(client: &dyn RpcClient) -> Option<String> {
    if client.is_remote() {
        return None;
    }
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lane_params(
    tab_id: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    destination: Option<String>,
    cleanup_status: Option<String>,
    cleanup_hints: Vec<String>,
) -> Option<Value> {
    let mut lane = json!({});
    if let Some(value) = tab_id {
        lane["tab_id"] = json!(value);
    }
    if let Some(value) = workspace_id {
        lane["workspace_id"] = json!(value);
    }
    if let Some(value) = workspace_name {
        lane["workspace_name"] = json!(value);
    }
    if let Some(value) = worktree_path {
        lane["worktree_path"] = json!(value);
    }
    if let Some(value) = branch {
        lane["branch"] = json!(value);
    }
    if let Some(value) = base {
        lane["base"] = json!(value);
    }
    if let Some(value) = destination {
        lane["destination"] = json!(value);
    }
    if let Some(value) = cleanup_status {
        lane["cleanup_status"] = json!(value);
    }
    if !cleanup_hints.is_empty() {
        lane["cleanup_hints"] = json!(cleanup_hints);
    }
    if lane.as_object().is_some_and(|items| !items.is_empty()) {
        Some(lane)
    } else {
        None
    }
}

pub fn handle_plan(client: &mut dyn RpcClient, json: bool, action: PlanAction) {
    match run_plan(client, json, action) {
        Ok(None) => {}
        Ok(Some(val)) => print_plan_result(json, &val),
        Err(e) => {
            crate::cmd::plan_output::print_plan_error(json, &e);
            std::process::exit(1);
        }
    }
}

fn run_plan(
    client: &mut dyn RpcClient,
    json: bool,
    action: PlanAction,
) -> Result<Option<Value>, String> {
    let result = match action {
        PlanAction::Create {
            title,
            from_spec,
            spec,
            spec_file,
            kinds,
            project,
        } => {
            let has_legacy_spec_shortcut = spec.is_some() || spec_file.is_some();
            if has_legacy_spec_shortcut {
                return Err(
                    "plan create no longer accepts --spec/--spec-file text input; create a real spec with `codeterm spec create` and use `codeterm plan create --from-spec <id> --title <title>`".to_string(),
                );
            }
            let mut params = json!({
                "title": title,
                "owner": owner_from_env(),
            });
            inject_trusted_session(&mut params);
            if let Some(k) = kinds {
                params["kinds"] = json!(k);
            }
            if let Some(project) = project.or_else(|| default_project_path(client)) {
                params["project_path"] = json!(project);
            }
            if let Some(spec_id) = from_spec {
                params["from_spec"] = json!(spec_id);
            }
            let mut val = call_plan_rpc(client, "plan.create", params)?;
            if val
                .get("from_spec")
                .and_then(|v| v.as_str())
                .is_none_or(str::is_empty)
            {
                push_hints(&mut val, vec![plan_create_spec_first_hint()]);
            }
            Ok(val)
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

        PlanAction::InProgress { plan_id, all } => {
            return crate::cmd::plan_in_progress::run(client, json, plan_id, all);
        }

        PlanAction::Get { plan_id, sections } => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_tab(&mut params);
            let val = call(client, "plan.get", params)?;
            if json {
                return Ok(Some(val));
            }
            if sections {
                print!("{}", render_plan_sections(&val));
            } else {
                println!("{}", render_plan_markdown(&val));
            }
            return Ok(None);
        }

        PlanAction::Spec { plan_id } => {
            let val = call(client, "plan.spec", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            print!(
                "{}",
                val.get("spec").and_then(|v| v.as_str()).unwrap_or_default()
            );
            return Ok(None);
        }

        PlanAction::Locate {
            plan_id,
            task,
            section,
            spec_section,
        } => {
            let mut params = json!({ "plan_id": plan_id });
            if let Some(t) = task {
                params["task"] = json!(t);
            }
            if let Some(s) = section {
                params["section"] = json!(s);
            }
            if let Some(s) = spec_section {
                params["spec_section"] = json!(s);
            }
            let val = call(client, "plan.locate", params)?;
            if json {
                return Ok(Some(val));
            }
            let file = val.get("file").and_then(|v| v.as_str()).unwrap_or("?");
            let start = val.get("start_line").and_then(|v| v.as_u64()).unwrap_or(0);
            let end = val.get("end_line").and_then(|v| v.as_u64()).unwrap_or(0);
            let editable = val
                .get("editable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            println!("{file}:{start}-{end}");
            if editable {
                println!("body is a yaml block scalar (markdown); edit these lines in place, keep the indentation");
            } else {
                println!(
                    "body is empty/inline — set it via CLI (--body/--body-file) instead of editing"
                );
            }
            return Ok(None);
        }

        PlanAction::Files { plan_id } => {
            let val = call(client, "plan.files", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", crate::cmd::plan_files::render_file_manifest(&val));
            return Ok(None);
        }

        PlanAction::Tags { plan_id } => {
            let val = call(client, "plan.tags", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", crate::cmd::plan_tags::render_tag_report(&val));
            return Ok(None);
        }

        PlanAction::State { plan_id } => return read_cmd::state(client, json, plan_id),

        PlanAction::Lanes { plan_id, stale } => {
            return read_cmd::lanes(client, json, plan_id, stale)
        }

        PlanAction::Next { plan_id } => {
            let val = call(client, "plan.next", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", crate::cmd::plan_next::render_next(&val));
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

        PlanAction::Gaps { plan_id } => {
            let val = call(client, "plan.coverage", json!({ "plan_id": plan_id }))?;
            if json {
                return Ok(Some(val));
            }
            let gaps: Vec<&str> = val
                .get("gaps")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            if gaps.is_empty() {
                println!("Gaps: none");
            } else {
                println!("{}", gaps.join("\n"));
            }
            let unlinked_count = val
                .get("unlinked_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if unlinked_count > 0 {
                println!("Unlinked tasks: {unlinked_count}");
            }
            return Ok(None);
        }

        PlanAction::Reconcile { plan_id, fix } => {
            let val = call(
                client,
                "plan.reconcile",
                json!({
                    "plan_id": plan_id,
                    "fix": fix,
                    "caller_tab_id": std::env::var("CODETERM_TAB_ID").ok(),
                }),
            )?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", crate::cmd::plan_reconcile::render_reconcile(&val));
            return Ok(None);
        }

        PlanAction::Doctor {
            plan_id,
            project,
            kind,
        } => {
            return crate::cmd::plan_doctor::run(client, json, plan_id, project, kind);
        }

        PlanAction::Prune { project, apply } => {
            return crate::cmd::plan_prune::run(client, json, apply, project);
        }

        PlanAction::Adopt {
            source,
            project,
            dry_run,
            overwrite,
        } => {
            let val = call(
                client,
                "plan.adopt",
                json!({
                    "source": source,
                    "project": project,
                    "owner": owner_from_env(),
                    "dry_run": dry_run,
                    "overwrite": overwrite,
                    "remove_source": false,
                }),
            )?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", render_adopt_report(&val));
            return Ok(None);
        }

        PlanAction::Move {
            plan_id,
            from,
            to,
            dry_run,
            overwrite,
        } => {
            let source = resolve_source_plan(&from, &plan_id)?;
            let val = call(
                client,
                "plan.adopt",
                json!({
                    "source": source,
                    "project": to,
                    "owner": owner_from_env(),
                    "dry_run": dry_run,
                    "overwrite": overwrite,
                    "remove_source": true,
                }),
            )?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", render_adopt_report(&val));
            return Ok(None);
        }

        PlanAction::Goal { action } => match action {
            PlanGoalAction::Show { plan_id } => {
                let val = call(client, "plan.goal.show", json!({ "plan_id": plan_id }))?;
                if json {
                    return Ok(Some(val));
                }
                if let Some(goal) = val.get("goal").filter(|v| !v.is_null()) {
                    let mut out = String::new();
                    render_goal_block(goal, &mut out);
                    print!("{out}");
                } else {
                    println!("(no goal)");
                }
                return Ok(None);
            }
            PlanGoalAction::Set {
                plan_id,
                goal_text,
                vector,
                vision,
                vision_file,
                constraint,
                postulate,
            } => {
                let vision = resolve_text(vision, vision_file)?
                    .or(goal_text)
                    .unwrap_or_default();
                let mut params = json!({
                    "plan_id": plan_id,
                    "vector": vector,
                    "vision": vision,
                    "constraints": constraint,
                    "postulates": postulate,
                });
                maybe_inject_caller_tab(&mut params);
                call(client, "plan.goal.set", params)
            }
        },

        PlanAction::Related { id, depth } => {
            call(client, "plan.related", json!({ "id": id, "depth": depth }))
        }

        PlanAction::Search {
            query,
            project,
            kind,
            scope,
            limit,
            active: _,
            archived,
            all,
        } => {
            let status = if all {
                "all"
            } else if archived {
                "archived"
            } else {
                "active"
            };
            let mut params = json!({
                "q": query,
                "limit": limit,
                "status": status,
            });
            if let Some(project) = project.or_else(|| default_project_path(client)) {
                params["project"] = json!(project);
            }
            if let Some(kind) = kind {
                params["kind"] = json!(kind);
            }
            if let Some(scope) = scope {
                params["scope"] = json!(scope);
            }
            let val = call(client, "plan.search", params)?;
            if json {
                return Ok(Some(val));
            }
            print!("{}", render_plan_search(&val));
            return Ok(None);
        }

        PlanAction::Reindex { project } => {
            let mut params = json!({});
            if let Some(project) = project.or_else(|| default_project_path(client)) {
                params["project"] = json!(project);
            }
            call(client, "plan.reindex", params)
        }

        PlanAction::Groups { action } => match action {
            PlanGroupsAction::Set { plan_id, groups } => call(
                client,
                "plan.groups.set",
                json!({ "plan_id": plan_id, "groups": groups }),
            ),
            PlanGroupsAction::Close { plan_id, key, note } => {
                let mut params = json!({ "plan_id": plan_id, "key": key, "note": note });
                maybe_inject_caller_tab(&mut params);
                call(client, "plan.groups.close", params)
            }
        },

        PlanAction::Facets { action } => match action {
            PlanFacetsAction::Set { plan_id, facets } => call(
                client,
                "plan.tag_facets.set",
                json!({ "plan_id": plan_id, "facets": facets }),
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
            mode,
            planner,
            note,
        } => {
            let mut params = json!({ "plan_id": plan_id });
            if let Some(t) = title {
                params["title"] = json!(t);
            }
            if let Some(s) = status {
                params["status"] = json!(s);
            }
            if let Some(mode) = mode {
                params["mode"] = json!(mode);
            }
            if let Some(ref p) = planner {
                let planner_ref = if p == "none" {
                    let n = note.ok_or_else(|| {
                        "--note is required when --planner none (waiver must explain solo intent)".to_string()
                    })?;
                    json!({ "kind": "waived", "note": n })
                } else {
                    json!({ "kind": "tab", "tab_id": p })
                };
                params["planner"] = planner_ref;
            }
            call(client, "plan.update", params)
        }

        PlanAction::Approve { plan_id } => {
            crate::cmd::plan_doctor::assert_quality_ready(client, &plan_id)?;
            let mut params = json!({ "plan_id": plan_id, "mode": "approved" });
            inject_trusted_session(&mut params);
            call(client, "plan.mode.set", params)
        }

        PlanAction::Unapprove { plan_id } => {
            let mut params = json!({ "plan_id": plan_id, "mode": "planning" });
            inject_trusted_session(&mut params);
            call(client, "plan.mode.set", params)
        }

        PlanAction::Rebind { plan_id } => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            inject_trusted_session(&mut params);
            call(client, "plan.session.rebind", params)
        }

        PlanAction::PlanningState {
            plan_id,
            set,
            tab,
            note,
        } => {
            let mut params = json!({ "plan_id": plan_id, "state": set });
            if let Some(p) = tab {
                params["by_tab"] = json!(p);
            }
            if let Some(n) = note {
                params["note"] = json!(n);
            }
            inject_trusted_session(&mut params);
            call(client, "plan.planning.set", params)
        }

        PlanAction::Mode { plan_id, mode, set } => {
            let mode = set.or(mode).ok_or_else(|| {
                "plan mode requires a mode: pass it as a positional or with --set (planning | approved | execution | complete)".to_string()
            })?;
            let mut params = json!({ "plan_id": plan_id, "mode": mode });
            inject_trusted_session(&mut params);
            call(client, "plan.mode.set", params)
        }

        PlanAction::Pause { plan_id, reason } => call(
            client,
            "plan.pause",
            json!({ "plan_id": plan_id, "reason": reason, "from": owner_from_env() }),
        ),

        PlanAction::Resume { plan_id } => {
            call(client, "plan.resume", json!({ "plan_id": plan_id }))
        }

        PlanAction::Archive { plan_id } => {
            call(client, "plan.archive", json!({ "plan_id": plan_id }))
        }

        PlanAction::Unarchive { plan_id } => {
            call(client, "plan.unarchive", json!({ "plan_id": plan_id }))
        }

        PlanAction::Delete { plan_id, yes } => {
            call(client, "plan.delete", plan_delete_params(plan_id, yes)?)
        }

        PlanAction::Transfer {
            plan_id,
            to,
            reason,
        } => {
            let caller_tab_id = (!client.is_remote()).then(caller_tab_id).flatten();
            let params = plan_transfer_params(plan_id, to, reason, caller_tab_id.as_deref())?;
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

        PlanAction::Record {
            plan_id,
            task,
            upsert_task,
            name,
            sections,
            status,
            body_file,
            steps,
            links,
            key,
        } => crate::cmd::plan_record::record(
            client,
            plan_id,
            task,
            upsert_task,
            name,
            sections,
            status,
            body_file,
            steps,
            links,
            key,
        ),

        PlanAction::Retrospective {
            plan_id,
            body,
            body_file,
        } => {
            let text = resolve_text(body, body_file)?;
            let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
                return Err("retrospective body is required (--body or --body-file)".into());
            };
            let mut params = json!({ "plan_id": plan_id, "body": text });
            maybe_inject_caller_tab(&mut params);
            let val = call(client, "plan.retrospective", params)?;
            if json {
                return Ok(Some(val));
            }
            println!(
                "Recorded retrospective for plan {}",
                val.get("plan_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(plan_id.as_str())
            );
            crate::cmd::plan_output::print_mutation_tail(false, &val);
            return Ok(None);
        }

        PlanAction::Ingest { plan_id, markdown } => {
            let markdown = read_path_or_stdin(&markdown)?;
            let mut params = json!({ "plan_id": plan_id, "markdown": markdown });
            inject_trusted_session(&mut params);
            call(client, "plan.ingest", params)
        }

        PlanAction::Handoff { action } => {
            return crate::cmd::plan_handoff::handle(client, json, action);
        }

        PlanAction::Section { action } => {
            return crate::cmd::plan_sections::handle(client, json, action);
        }

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
            PlanLoopAction::Record {
                loop_id,
                note,
                stop,
                task,
            } => call(
                client,
                "plan.loop.record",
                json!({
                    "loop_id": loop_id,
                    "note": note,
                    "stop": stop,
                    "task_id": task,
                }),
            ),
        },

        PlanAction::Task { action } => return task_cmd::run_task(client, json, action),

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
                    params["from"] = json!({ "tab_id": f });
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

        PlanAction::Qa { action } => return qa_cmd::handle(client, json, action),

        PlanAction::Review { action } => return review_cmd::handle(client, json, action),

        PlanAction::Decision { action } => return gate_cmd::handle(client, json, action),

        PlanAction::Skill { action } => {
            return crate::cmd::plan_skill::handle(client, json, action)
        }

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
            PlanNoteAction::Update {
                plan_id,
                note_id,
                text,
                text_file,
            } => {
                let text_val = resolve_text(text, text_file)?.unwrap_or_default();
                call(
                    client,
                    "plan.note.update",
                    json!({ "plan_id": plan_id, "element_id": note_id, "text": text_val }),
                )
            }
            PlanNoteAction::Rm { plan_id, note_id } => call(
                client,
                "plan.element.rm",
                json!({ "plan_id": plan_id, "element_id": note_id }),
            ),
        },

        PlanAction::Element { action } => match action {
            PlanElementAction::List { plan_id, kind } => {
                let mut params = json!({ "plan_id": plan_id });
                if let Some(kind) = kind {
                    params["kind"] = json!(kind);
                }
                let val = call(client, "plan.element.list", params)?;
                if json {
                    return Ok(Some(val));
                }
                print!("{}", crate::cmd::plan_elements::render_element_list(&val));
                return Ok(None);
            }
            PlanElementAction::Show {
                plan_id,
                element_id,
            } => {
                let val = call(
                    client,
                    "plan.element.show",
                    json!({ "plan_id": plan_id, "element_id": element_id }),
                )?;
                if json {
                    return Ok(Some(val));
                }
                print!("{}", crate::cmd::plan_elements::render_element_show(&val));
                return Ok(None);
            }
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
            PlanElementAction::Update {
                plan_id,
                element_id,
                kind,
                content,
                content_file,
                link,
            } => {
                let content_text = resolve_text(content, content_file)?;
                let mut params = json!({ "plan_id": plan_id, "element_id": element_id });
                if let Some(kind) = kind {
                    params["kind"] = json!(kind);
                }
                if let Some(c) = content_text {
                    params["content"] = json!({ "text": c });
                }
                if let Some(l) = link {
                    params["links"] = json!(l.split(',').map(str::trim).collect::<Vec<_>>());
                }
                call(client, "plan.element.update", params)
            }
            PlanElementAction::Rm {
                plan_id,
                element_id,
            } => call(
                client,
                "plan.element.rm",
                json!({ "plan_id": plan_id, "element_id": element_id }),
            ),
        },
    };

    result.map(Some)
}

#[cfg(test)]
mod tests;
