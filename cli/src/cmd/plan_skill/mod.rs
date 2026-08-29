use serde_json::{json, Value};

use crate::args::plan::PlanSkillAction;
use crate::client::RpcClient;

pub mod ledger;
pub mod mem;
pub mod store;

pub fn handle(
    client: &mut dyn RpcClient,
    json: bool,
    action: PlanSkillAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanSkillAction::New {
            slug,
            plan,
            title,
            area,
            check,
            rationale,
        } => new_skill(client, json, slug, plan, title, area, check, rationale),
        PlanSkillAction::List { plan } => list_skills(client, json, plan),
        PlanSkillAction::Show { slug, plan } => show_skill(client, json, slug, plan),
        PlanSkillAction::Edit {
            slug,
            plan,
            verdict,
            rationale,
            body_file,
        } => edit_skill(client, json, slug, plan, verdict, rationale, body_file),
    }
}

// Record the sidecar ledger entry in the plan timeline via a task-less stride.
// Sidecar write already succeeded; a failure here must be surfaced, not swallowed.
fn record_stride(
    client: &mut dyn RpcClient,
    plan_id: &str,
    slug: &str,
    verdict: &str,
    rationale: &str,
) -> bool {
    let text = format!("skill {slug} [{verdict}] {rationale}");
    let params = json!({
        "plan_id": plan_id,
        "kind": "skill",
        "text": text,
        "from": { "tab_id": std::env::var("CODETERM_TAB_ID").ok() },
    });
    match client.call("plan.stride.add", Some(params)) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("warning: sidecar ledger written but plan-timeline stride failed: {e}");
            false
        }
    }
}

fn resolve_local(
    client: &mut dyn RpcClient,
    plan: Option<String>,
) -> Result<(String, std::path::PathBuf), String> {
    let response = call(client, "plan.files", plan_rpc_params(plan))?;
    let index_path = response
        .get("index_path")
        .and_then(Value::as_str)
        .ok_or_else(|| "plan.files returned no authoritative index_path".to_string())?;
    let plan_dir = std::path::PathBuf::from(index_path)
        .parent()
        .ok_or_else(|| "authoritative plan index has no parent directory".to_string())?
        .to_path_buf();
    let plan_id = plan_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            if name == "_inbox" {
                "inbox".to_string()
            } else {
                name.to_string()
            }
        })
        .ok_or_else(|| "could not determine authoritative plan id".to_string())?;
    Ok((plan_id, plan_dir))
}

fn plan_rpc_params(plan: Option<String>) -> Value {
    let plan_id = plan.filter(|value| !value.trim().is_empty()).or_else(|| {
        std::env::var("CODETERM_PLAN_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let mut params = json!({});
    if let Some(plan_id) = plan_id {
        params["plan_id"] = json!(plan_id);
    } else {
        crate::cmd::plan::maybe_inject_caller_tab(&mut params);
    }
    params
}

fn call(client: &mut dyn RpcClient, method: &str, params: Value) -> Result<Value, String> {
    crate::cmd::plan::call_plan_rpc(client, method, params)
}

fn skill_value<'a>(response: &'a Value, method: &str) -> Result<&'a Value, String> {
    response
        .get("skill")
        .ok_or_else(|| format!("{method} returned no skill"))
}

fn best_effort_index(plan_id: &str, plan_dir: &std::path::Path, slug: &str) -> bool {
    match mem::index_skill(plan_id, plan_dir, slug) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("note: mem indexing skipped ({e})");
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn new_skill(
    client: &mut dyn RpcClient,
    json: bool,
    slug: String,
    plan: Option<String>,
    title: Option<String>,
    area: Option<String>,
    check: Option<String>,
    rationale: Option<String>,
) -> Result<Option<Value>, String> {
    let mut params = plan_rpc_params(plan);
    params["slug"] = json!(slug);
    if let Some(value) = title {
        params["title"] = json!(value);
    }
    if let Some(value) = area {
        params["area"] = json!(value);
    }
    if let Some(value) = check {
        params["check"] = json!(value);
    }
    if let Some(value) = rationale {
        params["rationale"] = json!(value);
    }
    let response = call(client, "plan.skills.new", params)?;
    if json {
        return Ok(Some(response));
    }
    let skill = skill_value(&response, "plan.skills.new")?;
    println!(
        "Created plan skill '{}' for plan {}",
        skill["slug"].as_str().unwrap_or(""),
        response["plan_id"].as_str().unwrap_or("")
    );
    println!("  SKILL.md: {}", skill["skill_md"].as_str().unwrap_or(""));
    println!("  scripts:  {}", skill["scripts"].as_str().unwrap_or(""));
    println!(
        "  ledger:   seq {} verdict={}",
        skill["ledger_seq"],
        skill["verdict"].as_str().unwrap_or("")
    );
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn edit_skill(
    client: &mut dyn RpcClient,
    json: bool,
    slug: String,
    plan: Option<String>,
    verdict: String,
    rationale: String,
    body_file: Option<String>,
) -> Result<Option<Value>, String> {
    let (plan_id, plan_dir) = resolve_local(client, plan)?;
    let slug = store::slugify(&slug);
    let skill_md = store::skill_md_path(&plan_dir, &slug);
    if !skill_md.is_file() {
        return Err(format!(
            "plan skill '{slug}' not found; run `codeterm plan skill new {slug}`"
        ));
    }
    if let Some(path) = body_file {
        let body = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
        std::fs::write(&skill_md, body).map_err(|e| e.to_string())?;
    }
    let entry = ledger::append_entry(&plan_dir, &slug, &verdict, &rationale)?;
    let timeline = record_stride(client, &plan_id, &slug, &entry.verdict, &rationale);
    let indexed = best_effort_index(&plan_id, &plan_dir, &slug);

    if json {
        return Ok(Some(json!({
            "plan_id": plan_id,
            "slug": slug,
            "ledger_seq": entry.seq,
            "verdict": entry.verdict,
            "timeline_recorded": timeline,
            "mem_indexed": indexed,
        })));
    }
    println!(
        "Recorded {} verdict for plan skill '{slug}' (seq {})",
        entry.verdict, entry.seq
    );
    Ok(None)
}

fn list_skills(
    client: &mut dyn RpcClient,
    json: bool,
    plan: Option<String>,
) -> Result<Option<Value>, String> {
    let response = call(client, "plan.skills.list", plan_rpc_params(plan))?;
    if json {
        return Ok(Some(response));
    }
    let rows = response["skills"]
        .as_array()
        .ok_or_else(|| "plan.skills.list returned no skills array".to_string())?;
    if rows.is_empty() {
        println!(
            "No plan skills for plan {}.",
            response["plan_id"].as_str().unwrap_or("")
        );
        return Ok(None);
    }
    println!(
        "Plan skills for {}:",
        response["plan_id"].as_str().unwrap_or("")
    );
    for row in rows {
        println!(
            "  - {} [{}]",
            row["slug"].as_str().unwrap_or(""),
            row["verdict"].as_str().unwrap_or("-")
        );
    }
    Ok(None)
}

fn show_skill(
    client: &mut dyn RpcClient,
    json: bool,
    slug: String,
    plan: Option<String>,
) -> Result<Option<Value>, String> {
    let mut params = plan_rpc_params(plan);
    params["slug"] = json!(slug);
    let binding = [
        std::env::var("CODETERM_TAB_ID").ok(),
        std::env::var("CODETERM_SESSION_ID").ok(),
        std::env::var("CODETERM_SESSION_BINDING_NONCE").ok(),
    ]
    .map(|value| value.filter(|value| !value.trim().is_empty()));
    if binding.iter().any(Option::is_some) && binding.iter().any(Option::is_none) {
        return Err("plan skill show requires complete tab/session/nonce identity".into());
    }
    if let [Some(tab_id), Some(session_id), Some(session_binding_nonce)] = binding {
        params["caller_tab_id"] = json!(tab_id);
        params["caller_session_id"] = json!(session_id);
        params["session_binding_nonce"] = json!(session_binding_nonce);
        if let Ok(task_id) = std::env::var("CODETERM_TASK_ID") {
            if !task_id.trim().is_empty() {
                params["task_id"] = json!(task_id);
            }
        }
        if let Ok(attempt_id) = std::env::var("CODETERM_ATTEMPT_ID") {
            if !attempt_id.trim().is_empty() {
                params["attempt_id"] = json!(attempt_id);
            }
        }
    } else if ["CODETERM_TASK_ID", "CODETERM_ATTEMPT_ID"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|value| !value.trim().is_empty()))
    {
        return Err("plan skill show requires complete tab/session/nonce identity".into());
    }
    let response = call(client, "plan.skills.show", params)?;
    if json {
        return Ok(Some(response));
    }
    let skill = skill_value(&response, "plan.skills.show")?;
    print!("{}", skill["body"].as_str().unwrap_or(""));
    println!("\n--- ledger ---");
    for entry in skill["ledger"].as_array().into_iter().flatten() {
        println!(
            "  seq {} [{}] {}",
            entry["seq"],
            entry["verdict"].as_str().unwrap_or(""),
            entry["rationale"].as_str().unwrap_or("")
        );
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
