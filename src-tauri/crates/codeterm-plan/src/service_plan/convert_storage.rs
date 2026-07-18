//! One-off legacy → store_v2 migration. Deep-compare is the deliverable;
//! conversion is the side effect. Never deletes on mismatch.

use crate::plan::{Plan, PlanStatus, Section};
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use super::{body_files, migration_backup, paths, store_v2, tag_model};

#[derive(Debug)]
pub struct ConvertError {
    pub plan_id: String,
    pub plan_dir: PathBuf,
    pub message: String,
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "plan {} ({}): {}",
            self.plan_id,
            self.plan_dir.display(),
            self.message
        )
    }
}

impl std::error::Error for ConvertError {}

pub struct Args {
    pub project: PathBuf,
    pub dry_run: bool,
    pub plan_id: Option<String>,
    pub restore_plan_id: Option<String>,
}

pub fn parse_args() -> Result<Args, String> {
    let mut project = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut dry_run = false;
    let mut plan_id = None;
    let mut restore_plan_id = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--project" => {
                project = PathBuf::from(it.next().ok_or("--project needs value")?);
            }
            "--dry-run" => dry_run = true,
            "--plan-id" => {
                plan_id = Some(it.next().ok_or("--plan-id needs value")?);
            }
            "--restore-plan-id" => {
                restore_plan_id = Some(it.next().ok_or("--restore-plan-id needs value")?);
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        project,
        dry_run,
        plan_id,
        restore_plan_id,
    })
}

fn print_usage() {
    eprintln!(
        "convert-plan-storage — legacy plan.json sidecars → store_v2 yaml (one-off)\n\
         \n\
         Usage:\n\
           cargo run -p codeterm --bin convert-plan-storage -- [options]\n\
         \n\
         Options:\n\
           --project <path>   project root (default: cwd)\n\
           --plan-id <id>     convert one plan only\n\
           --restore-plan-id <id>  restore a pre-v2 snapshot after plan loss\n\
           --dry-run          write + deep-compare, never delete legacy files\n\
           --help             show this help\n\
         \n\
         Rehearse on a COPY of the project first. Exits non-zero on any mismatch."
    );
}

pub fn run(args: Args) -> Result<(), ConvertError> {
    let project_root = paths::project_root(&args.project);
    if let Some(plan_id) = args.restore_plan_id {
        if args.plan_id.is_some() || args.dry_run {
            return Err(ConvertError {
                plan_id,
                plan_dir: project_root,
                message: "--restore-plan-id cannot be combined with conversion options".into(),
            });
        }
        let restored =
            migration_backup::restore(&project_root, &plan_id).map_err(|e| ConvertError {
                plan_id: plan_id.clone(),
                plan_dir: migration_backup::backup_dir(&project_root, &plan_id),
                message: format!("restore failed: {e}"),
            })?;
        eprintln!("[convert] restored {plan_id} to {}", restored.display());
        return Ok(());
    }
    let plans = discover_legacy_plan_dirs(&project_root, args.plan_id.as_deref()).map_err(|e| {
        ConvertError {
            plan_id: String::new(),
            plan_dir: project_root.clone(),
            message: format!("discover failed: {e}"),
        }
    })?;
    if plans.is_empty() {
        eprintln!(
            "[convert] no legacy plan.json directories under {}",
            project_root.join(".codeterm/plans").display()
        );
        return Ok(());
    }
    eprintln!(
        "[convert] project={} plans={} dry_run={}",
        project_root.display(),
        plans.len(),
        args.dry_run
    );
    let mut failures = 0usize;
    for plan_dir in plans {
        if let Err(e) = convert_plan_dir(&plan_dir, &project_root, args.dry_run) {
            eprintln!("\n╔══════════════════════════════════════════════════════════╗");
            eprintln!("║  CONVERSION FAILED — legacy files left untouched         ║");
            eprintln!("╚══════════════════════════════════════════════════════════╝");
            eprintln!("{e}");
            failures += 1;
        }
    }
    if failures > 0 {
        return Err(ConvertError {
            plan_id: "(batch)".into(),
            plan_dir: project_root,
            message: format!("{failures} plan(s) failed deep-compare or I/O"),
        });
    }
    eprintln!("[convert] all plans passed deep-compare");
    Ok(())
}

pub fn main() -> Result<(), ConvertError> {
    run(parse_args().map_err(|e| ConvertError {
        plan_id: String::new(),
        plan_dir: PathBuf::new(),
        message: e,
    })?)
}

pub fn discover_legacy_plan_dirs(
    project_root: &Path,
    only_id: Option<&str>,
) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let plans_root = project_root.join(".codeterm").join("plans");
    for bucket in ["active", "archive"] {
        let bucket_dir = plans_root.join(bucket);
        let Ok(entries) = std::fs::read_dir(&bucket_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let plan_dir = entry.path();
            if !plan_dir.is_dir() {
                continue;
            }
            let plan_json = plan_dir.join("plan.json");
            if !plan_json.is_file() {
                continue;
            }
            if let Some(id) = only_id {
                let stem = plan_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if stem != id {
                    continue;
                }
            }
            out.push(plan_dir);
        }
    }
    out.sort();
    Ok(out)
}

pub fn load_legacy_plan(plan_json: &Path, project_root: &Path) -> io::Result<Plan> {
    let json = std::fs::read_to_string(plan_json)?;
    let mut plan: Plan =
        serde_json::from_str(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    paths::normalize_plan_paths(&mut plan, project_root);
    body_files::hydrate_and_migrate(&mut plan)?;
    tag_model::normalize_plan(&mut plan);
    Ok(plan)
}

pub fn normalize_for_compare(plan: &mut Plan) {
    tag_model::normalize_plan(plan);
    plan.tasks.sort_by(|a, b| a.id.cmp(&b.id));
    for task in &mut plan.tasks {
        task.milestones.sort_by(|a, b| a.id.cmp(&b.id));
        task.execution_attempts.sort_by(|a, b| a.id.cmp(&b.id));
        task.attachments.sort_by(|a, b| a.id.cmp(&b.id));
        task.section_ids.sort();
        task.tags.sort();
        task.acceptance_criteria.sort();
    }
    sort_sections(&mut plan.sections);
    plan.elements.sort_by(|a, b| a.id.cmp(&b.id));
    plan.loops.sort_by(|a, b| a.id.cmp(&b.id));
    plan.task_groups.sort_by(|a, b| a.key.cmp(&b.key));
    plan.spec_links.sort_by(|a, b| {
        (&a.spec_id, &a.spec_section_id, &a.plan_section_id).cmp(&(
            &b.spec_id,
            &b.spec_section_id,
            &b.plan_section_id,
        ))
    });
    plan.spec_refs.sort();
    plan.links.sort();
    plan.strides.sort_by(|a, b| a.id.cmp(&b.id));
    plan.planning_log.sort_by(|a, b| a.at.cmp(&b.at));
    plan.gates.sort_by(|a, b| a.id.cmp(&b.id));
}

fn sort_sections(sections: &mut [Section]) {
    sections.sort_by(|a, b| a.id.cmp(&b.id));
    for section in sections.iter_mut() {
        section.tags.sort();
        section.attachments.sort_by(|a, b| a.id.cmp(&b.id));
        sort_sections(&mut section.subsections);
    }
}

pub fn deep_compare_plans(legacy: &Plan, loaded: &Plan) -> Result<(), String> {
    let mut left = legacy.clone();
    let mut right = loaded.clone();
    normalize_for_compare(&mut left);
    normalize_for_compare(&mut right);
    if left == right {
        return Ok(());
    }
    let legacy_json = serde_json::to_string_pretty(&left)
        .map_err(|e| format!("serialize legacy for diff: {e}"))?;
    let loaded_json = serde_json::to_string_pretty(&right)
        .map_err(|e| format!("serialize loaded for diff: {e}"))?;
    Err(format_diff(&legacy_json, &loaded_json))
}

fn format_diff(legacy: &str, loaded: &str) -> String {
    let mut out = String::from("\n── DEEP-COMPARE MISMATCH ──\n");
    let legacy_lines: Vec<&str> = legacy.lines().collect();
    let loaded_lines: Vec<&str> = loaded.lines().collect();
    let max = legacy_lines.len().max(loaded_lines.len());
    let mut first_diff = None;
    for i in 0..max {
        let l = legacy_lines.get(i).copied().unwrap_or("<missing>");
        let r = loaded_lines.get(i).copied().unwrap_or("<missing>");
        if l != r && first_diff.is_none() {
            first_diff = Some(i);
        }
    }
    if let Some(start) = first_diff {
        let from = start.saturating_sub(3);
        let to = (start + 12).min(max);
        out.push_str(&format!(
            "first differing line near {} (showing {}..{}):\n",
            start + 1,
            from + 1,
            to
        ));
        for i in from..to {
            let l = legacy_lines.get(i).copied().unwrap_or("");
            let r = loaded_lines.get(i).copied().unwrap_or("");
            let mark = if l != r { ">>>" } else { "   " };
            out.push_str(&format!("{mark} L{:04} | {}\n", i + 1, l));
            out.push_str(&format!("{mark} R{:04} | {}\n", i + 1, r));
        }
    }
    out.push_str("\n(full normalized legacy JSON written to stderr context above)\n");
    out
}

pub fn stride_log_snapshots(plan_dir: &Path, plan: &Plan) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let mut snaps = BTreeMap::new();
    for task in &plan.tasks {
        let handle = tag_model::task_handle(task);
        let nested = plan_dir.join("tasks").join(handle).join("strides.jsonl");
        if nested.is_file() {
            let key = nested.strip_prefix(plan_dir).unwrap().display().to_string();
            snaps.insert(key, std::fs::read(&nested)?);
        }
        let flat = plan_dir.join("tasks").join(format!("{handle}.jsonl"));
        if flat.is_file() {
            let key = flat.strip_prefix(plan_dir).unwrap().display().to_string();
            snaps.insert(key, std::fs::read(&flat)?);
        }
    }
    Ok(snaps)
}

pub fn verify_stride_logs_unchanged(
    before: &BTreeMap<String, Vec<u8>>,
    plan_dir: &Path,
) -> Result<(), String> {
    for (rel, bytes) in before {
        let path = plan_dir.join(rel);
        let after = std::fs::read(&path)
            .map_err(|e| format!("stride log {} missing after write: {e}", path.display()))?;
        if after != *bytes {
            return Err(format!(
                "stride log {} changed during conversion ({} → {} bytes)",
                path.display(),
                bytes.len(),
                after.len()
            ));
        }
    }
    Ok(())
}

pub fn convert_plan_dir(
    plan_dir: &Path,
    project_root: &Path,
    dry_run: bool,
) -> Result<(), ConvertError> {
    let plan_json = plan_dir.join("plan.json");
    let plan_id = plan_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    eprintln!("[convert] {plan_id} ← {}", plan_dir.display());

    let legacy = load_legacy_plan(&plan_json, project_root).map_err(|e| ConvertError {
        plan_id: plan_id.clone(),
        plan_dir: plan_dir.to_path_buf(),
        message: format!("legacy load failed: {e}"),
    })?;
    if legacy.id != plan_id && plan_id != crate::plan::INBOX_PLAN_FILE_STEM {
        return Err(ConvertError {
            plan_id: plan_id.clone(),
            plan_dir: plan_dir.to_path_buf(),
            message: format!("directory name {plan_id} != plan.id {}", legacy.id),
        });
    }

    let strides_before = stride_log_snapshots(plan_dir, &legacy).map_err(|e| ConvertError {
        plan_id: plan_id.clone(),
        plan_dir: plan_dir.to_path_buf(),
        message: format!("stride snapshot failed: {e}"),
    })?;

    store_v2::write_plan(plan_dir, &legacy).map_err(|e| ConvertError {
        plan_id: plan_id.clone(),
        plan_dir: plan_dir.to_path_buf(),
        message: format!("store_v2 write failed: {e}"),
    })?;

    verify_stride_logs_unchanged(&strides_before, plan_dir).map_err(|e| ConvertError {
        plan_id: plan_id.clone(),
        plan_dir: plan_dir.to_path_buf(),
        message: e,
    })?;

    let loaded = store_v2::load_plan(plan_dir).map_err(|e| ConvertError {
        plan_id: plan_id.clone(),
        plan_dir: plan_dir.to_path_buf(),
        message: format!("store_v2 load failed: {e}"),
    })?;

    if let Err(diff) = deep_compare_plans(&legacy, &loaded) {
        eprintln!("{diff}");
        eprintln!(
            "\n╔══════════════════════════════════════════════════════════╗\n\
             ║  MISMATCH — NOT deleting legacy files for {plan_id:<22} ║\n\
             ╚══════════════════════════════════════════════════════════╝"
        );
        return Err(ConvertError {
            plan_id,
            plan_dir: plan_dir.to_path_buf(),
            message: "deep-compare failed".into(),
        });
    }

    eprintln!("[convert] {plan_id} deep-compare OK");

    if dry_run {
        eprintln!("[convert] {plan_id} --dry-run: skipping legacy cleanup");
        return Ok(());
    }

    migration_backup::create_backup(project_root, &plan_id, plan_dir).map_err(|e| {
        ConvertError {
            plan_id: plan_id.clone(),
            plan_dir: plan_dir.to_path_buf(),
            message: format!("recovery snapshot failed: {e}"),
        }
    })?;

    remove_legacy_artifacts(plan_dir, &legacy).map_err(|e| ConvertError {
        plan_id: plan_id.clone(),
        plan_dir: plan_dir.to_path_buf(),
        message: format!("legacy cleanup failed: {e}"),
    })?;
    eprintln!("[convert] {plan_id} legacy artifacts removed");
    Ok(())
}

pub fn remove_legacy_artifacts(plan_dir: &Path, plan: &Plan) -> io::Result<()> {
    remove_file_if_exists(&plan_dir.join("plan.json"))?;
    remove_file_if_exists(&plan_dir.join("INDEX.md"))?;
    remove_file_if_exists(&plan_dir.join("goal.md"))?;
    remove_dir_if_exists(&plan_dir.join("sections"))?;
    remove_dir_if_exists(&plan_dir.join("milestones"))?;
    remove_dir_if_exists(&plan_dir.join("elements"))?;
    remove_dir_if_exists(&plan_dir.join("handoffs"))?;
    let tasks_dir = plan_dir.join("tasks");
    if tasks_dir.is_dir() {
        for entry in std::fs::read_dir(&tasks_dir)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                std::fs::remove_file(&path)?;
            }
        }
    }
    for task in &plan.tasks {
        let handle = tag_model::task_handle(task);
        let milestone_dir_file = plan_dir.join("milestones");
        if milestone_dir_file.is_dir() {
            for milestone in &task.milestones {
                let name = format!("{handle}-{}.md", milestone.id);
                remove_file_if_exists(&milestone_dir_file.join(name))?;
            }
        }
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn remove_dir_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn bucket_status(plan_dir: &Path) -> PlanStatus {
    plan_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .filter(|b| *b == "archive")
        .map(|_| PlanStatus::Archived)
        .unwrap_or(PlanStatus::Active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Goal, OrchestratorRef, PlanStatus};
    use crate::service_plan::{NewTask, PlanStore};

    fn legacy_fixture_plan(dir: &Path) -> Plan {
        let store = PlanStore::new_for_test(dir);
        let mut plan = Plan::new(
            "Convert fixture".into(),
            dir.display().to_string(),
            OrchestratorRef::default(),
        );
        plan.goal = Some(Goal {
            vector: "north".into(),
            vision: "multi\nline\nvision".into(),
            constraints: vec!["c1".into()],
            postulates: vec![],
        });
        store.save(&mut plan, 1).unwrap();
        store
            .add_section(&plan.id, "Phase", "section\nbody", None, &[], None, None, 2)
            .unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Ship it".into(),
                    handle: Some("ship-it".into()),
                    body: "task\nbody".into(),
                    acceptance_criteria: vec!["works".into()],
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        store
            .update_task(
                &plan.id,
                &task_id,
                None,
                None,
                None,
                None,
                Some("done"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                4,
            )
            .unwrap();
        // The store now persists yaml; this fixture must be pure LEGACY
        // (plan.json + md sidecars) for the converter to convert. Rewrite it
        // in legacy form and strip the yaml the store wrote.
        let final_plan = store.get(&plan.id).unwrap();
        let plan_dir = dir.join(format!(".codeterm/plans/active/{}", final_plan.id));
        let _ = std::fs::remove_file(plan_dir.join("plan.yaml"));
        let _ = std::fs::remove_file(plan_dir.join("goal.yaml"));
        for f in glob_yaml(&plan_dir.join("tasks")) {
            let _ = std::fs::remove_file(f);
        }
        PlanStore::write_atomic(&final_plan).unwrap();
        final_plan
    }

    fn glob_yaml(dir: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yaml"))
            .collect()
    }

    #[test]
    fn round_trip_passes_deep_compare_on_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let plan = legacy_fixture_plan(dir.path());
        let plan_dir = dir
            .path()
            .join(format!(".codeterm/plans/active/{}", plan.id));
        convert_plan_dir(&plan_dir, dir.path(), true).unwrap();
        assert!(plan_dir.join("plan.yaml").is_file());
        assert!(plan_dir.join("goal.yaml").is_file());
        assert!(plan_dir.join("tasks/ship-it.yaml").is_file());
        assert!(plan_dir.join("plan.json").is_file(), "dry-run keeps legacy");
    }

    #[test]
    fn deep_compare_fails_when_v2_yaml_is_mangled() {
        let dir = tempfile::tempdir().unwrap();
        let plan = legacy_fixture_plan(dir.path());
        let plan_dir = dir
            .path()
            .join(format!(".codeterm/plans/active/{}", plan.id));
        let legacy = load_legacy_plan(&plan_dir.join("plan.json"), dir.path()).unwrap();
        store_v2::write_plan(&plan_dir, &legacy).unwrap();
        let task_yaml = plan_dir.join("tasks/ship-it.yaml");
        let mut text = std::fs::read_to_string(&task_yaml).unwrap();
        text = text.replace("Ship it", "CORRUPTED NAME");
        std::fs::write(&task_yaml, text).unwrap();
        let loaded = store_v2::load_plan(&plan_dir).unwrap();
        assert!(deep_compare_plans(&legacy, &loaded).is_err());
    }

    #[test]
    fn cleanup_removes_legacy_but_keeps_stride_logs() {
        let dir = tempfile::tempdir().unwrap();
        let plan = legacy_fixture_plan(dir.path());
        let plan_dir = dir
            .path()
            .join(format!(".codeterm/plans/active/{}", plan.id));
        let stride_path = plan_dir.join("tasks/ship-it/strides.jsonl");
        std::fs::create_dir_all(stride_path.parent().unwrap()).unwrap();
        std::fs::write(
            &stride_path,
            "{\"id\":\"ms_1\",\"kind\":\"note\",\"text\":\"hi\",\"ts\":1}\n",
        )
        .unwrap();
        convert_plan_dir(&plan_dir, dir.path(), false).unwrap();
        assert!(!plan_dir.join("plan.json").exists());
        assert!(!plan_dir.join("tasks/ship-it.md").exists());
        assert!(stride_path.is_file());
        assert!(plan_dir.join("tasks/ship-it.yaml").is_file());
    }

    #[test]
    fn active_plan_can_be_restored_from_its_pre_migration_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let plan = legacy_fixture_plan(dir.path());
        let plan_dir = dir
            .path()
            .join(format!(".codeterm/plans/active/{}", plan.id));

        convert_plan_dir(&plan_dir, dir.path(), false).unwrap();
        let backup = migration_backup::backup_dir(dir.path(), &plan.id);
        assert!(backup.join("plan.json").is_file());
        assert!(!plan_dir.join("plan.json").exists());

        std::fs::remove_dir_all(&plan_dir).unwrap();
        let restored = migration_backup::restore(dir.path(), &plan.id).unwrap();
        assert_eq!(restored, plan_dir);

        let store = PlanStore::new_for_test(dir.path());
        store.scan_project(dir.path()).unwrap();
        let restored_plan = store.get(&plan.id).expect("restored active plan");
        assert_eq!(restored_plan.status, PlanStatus::Active);
        assert_eq!(restored_plan.tasks.len(), plan.tasks.len());
        assert_eq!(restored_plan.tasks[0].name, "Ship it");
    }
}
