use serde_json::{json, Value};

use crate::args::plan::PlanSkillAction;
use crate::client::RpcClient;
use crate::cmd::plan_local::project_root;

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
        PlanSkillAction::List { plan } => list_skills(json, plan),
        PlanSkillAction::Show { slug, plan } => show_skill(json, slug, plan),
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
        "from": { "pane_id": std::env::var("CODETERM_PANE_ID").ok() },
    });
    match client.call("plan.stride.add", Some(params)) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("warning: sidecar ledger written but plan-timeline stride failed: {e}");
            false
        }
    }
}

fn resolve(plan: Option<String>) -> Result<(String, std::path::PathBuf), String> {
    let root = project_root(None)?;
    store::resolve_plan_dir(&root, plan)
}

fn titleize(slug: &str) -> String {
    slug.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    let (plan_id, plan_dir) = resolve(plan)?;
    let slug = store::slugify(&slug);
    if slug.is_empty() {
        return Err("slug is empty after normalization".to_string());
    }
    let dir = store::skill_dir(&plan_dir, &slug);
    if dir.exists() {
        return Err(format!(
            "plan skill '{slug}' already exists at {}",
            dir.display()
        ));
    }
    let title = title.unwrap_or_else(|| titleize(&slug));
    let area = area.unwrap_or_else(|| "general".to_string());
    std::fs::create_dir_all(dir.join("scripts")).map_err(|e| e.to_string())?;
    let skill_md = store::skill_md_path(&plan_dir, &slug);
    std::fs::write(
        &skill_md,
        store::scaffold(&slug, &title, &area, check.as_deref()),
    )
    .map_err(|e| e.to_string())?;

    let rationale = rationale.unwrap_or_else(|| "created".to_string());
    let entry = ledger::append_entry(&plan_dir, &slug, store::KEEP, &rationale)?;
    let timeline = record_stride(client, &plan_id, &slug, store::KEEP, &rationale);
    let indexed = best_effort_index(&plan_id, &plan_dir, &slug);

    if json {
        return Ok(Some(json!({
            "plan_id": plan_id,
            "slug": slug,
            "skill_md": skill_md.display().to_string(),
            "ledger_seq": entry.seq,
            "timeline_recorded": timeline,
            "mem_indexed": indexed,
        })));
    }
    println!("Created plan skill '{slug}' for plan {plan_id}");
    println!("  SKILL.md: {}", skill_md.display());
    println!("  scripts:  {}", dir.join("scripts").display());
    println!("  ledger:   seq {} verdict={}", entry.seq, entry.verdict);
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
    let (plan_id, plan_dir) = resolve(plan)?;
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

fn list_skills(json: bool, plan: Option<String>) -> Result<Option<Value>, String> {
    let (plan_id, plan_dir) = resolve(plan)?;
    let slugs = store::list_skills(&plan_dir);
    let rows: Vec<Value> = slugs
        .iter()
        .map(|slug| {
            let verdict = ledger::latest_verdict(&plan_dir, slug).unwrap_or_else(|| "-".into());
            json!({ "slug": slug, "verdict": verdict })
        })
        .collect();
    if json {
        return Ok(Some(json!({ "plan_id": plan_id, "skills": rows })));
    }
    if rows.is_empty() {
        println!("No plan skills for plan {plan_id}.");
        return Ok(None);
    }
    println!("Plan skills for {plan_id}:");
    for row in &rows {
        println!(
            "  - {} [{}]",
            row["slug"].as_str().unwrap_or(""),
            row["verdict"].as_str().unwrap_or("-")
        );
    }
    Ok(None)
}

fn show_skill(json: bool, slug: String, plan: Option<String>) -> Result<Option<Value>, String> {
    let (plan_id, plan_dir) = resolve(plan)?;
    let slug = store::slugify(&slug);
    let skill_md = store::skill_md_path(&plan_dir, &slug);
    if !skill_md.is_file() {
        return Err(format!("plan skill '{slug}' not found"));
    }
    let body = std::fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
    let history: Vec<Value> = ledger::read_ledger(&plan_dir)
        .into_iter()
        .filter(|entry| entry.slug == slug)
        .map(|entry| json!({ "seq": entry.seq, "verdict": entry.verdict, "rationale": entry.rationale }))
        .collect();
    if json {
        return Ok(Some(json!({
            "plan_id": plan_id,
            "slug": slug,
            "skill_md": skill_md.display().to_string(),
            "body": body,
            "ledger": history,
        })));
    }
    print!("{body}");
    println!("\n--- ledger ---");
    for entry in &history {
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
