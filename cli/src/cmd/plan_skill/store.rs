use std::path::{Path, PathBuf};

use crate::cmd::plan_local::{plan_files, plan_id_from};

pub const KEEP: &str = "keep";
pub const DISCARD: &str = "discard";
pub const SUPERSEDED: &str = "superseded";

pub fn valid_verdict(verdict: &str) -> bool {
    matches!(verdict, KEEP | DISCARD | SUPERSEDED)
}

pub fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn plans_dir(project_root: &Path) -> PathBuf {
    project_root.join(".codeterm").join("plans")
}

pub fn resolve_plan_dir(
    project_root: &Path,
    plan_id: Option<String>,
) -> Result<(String, PathBuf), String> {
    let plans = plans_dir(project_root);
    let requested = plan_id.filter(|s| !s.trim().is_empty()).or_else(|| {
        std::env::var("CODETERM_PLAN_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
    });

    if let Some(id) = requested {
        let files = plan_files(&plans, Some(&id))?;
        let path = files
            .into_iter()
            .next()
            .ok_or_else(|| format!("plan {id} not found under {}", plans.display()))?;
        let dir = path
            .parent()
            .ok_or_else(|| "plan.json has no parent dir".to_string())?
            .to_path_buf();
        return Ok((id, dir));
    }

    // Zero-flag: only the active bucket is eligible for auto-resolution.
    let active = plans.join("active");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&active) {
        for entry in entries.flatten() {
            let plan_yaml = entry.path().join("plan.yaml");
            if plan_yaml.is_file() {
                candidates.push(plan_yaml);
            }
        }
    }
    match candidates.len() {
        0 => Err(format!(
            "no active plans under {}; pass --plan <id>",
            active.display()
        )),
        1 => {
            let path = candidates.remove(0);
            let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let value: serde_json::Value = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
            let id = plan_id_from(&value, &path)
                .ok_or_else(|| "could not determine plan id".to_string())?;
            let dir = path.parent().unwrap().to_path_buf();
            Ok((id, dir))
        }
        n => Err(format!(
            "{n} active plans found; pass --plan <id> to disambiguate"
        )),
    }
}

pub fn skills_root(plan_dir: &Path) -> PathBuf {
    plan_dir.join("skills")
}

pub fn skill_dir(plan_dir: &Path, slug: &str) -> PathBuf {
    skills_root(plan_dir).join(slug)
}

pub fn skill_md_path(plan_dir: &Path, slug: &str) -> PathBuf {
    skill_dir(plan_dir, slug).join("SKILL.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_normalizes() {
        assert_eq!(slugify("Deploy To Staging"), "deploy-to-staging");
        assert_eq!(slugify("  weird__name!! "), "weird-name");
        assert_eq!(slugify("already-kebab"), "already-kebab");
        assert_eq!(slugify("MixedCASE123"), "mixedcase123");
    }

    #[test]
    fn verdict_validation() {
        assert!(valid_verdict("keep"));
        assert!(valid_verdict("discard"));
        assert!(valid_verdict("superseded"));
        assert!(!valid_verdict("maybe"));
    }

    fn write_plan(plans: &Path, bucket: &str, id: &str) -> PathBuf {
        let dir = plans.join(bucket).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plan.yaml"),
            serde_yaml::to_string(&serde_json::json!({ "id": id })).unwrap(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn resolve_explicit_plan_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let plans = plans_dir(root);
        write_plan(&plans, "active", "plan_one");
        let (id, dir) = resolve_plan_dir(root, Some("plan_one".into())).unwrap();
        assert_eq!(id, "plan_one");
        assert!(dir.ends_with("active/plan_one"));
    }

    #[test]
    fn resolve_single_active_when_omitted() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let previous = std::env::var("CODETERM_PLAN_ID").ok();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let plans = plans_dir(root);
        write_plan(&plans, "active", "solo");
        std::env::remove_var("CODETERM_PLAN_ID");
        let (id, _dir) = resolve_plan_dir(root, None).unwrap();
        assert_eq!(id, "solo");
        match previous {
            Some(value) => std::env::set_var("CODETERM_PLAN_ID", value),
            None => std::env::remove_var("CODETERM_PLAN_ID"),
        }
    }

    #[test]
    fn resolve_ambiguous_active_errors() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let previous = std::env::var("CODETERM_PLAN_ID").ok();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let plans = plans_dir(root);
        write_plan(&plans, "active", "a");
        write_plan(&plans, "active", "b");
        std::env::remove_var("CODETERM_PLAN_ID");
        let err = resolve_plan_dir(root, None).unwrap_err();
        assert!(err.contains("disambiguate"));
        match previous {
            Some(value) => std::env::set_var("CODETERM_PLAN_ID", value),
            None => std::env::remove_var("CODETERM_PLAN_ID"),
        }
    }

    #[test]
    fn resolve_missing_plan_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let err = resolve_plan_dir(root, Some("ghost".into())).unwrap_err();
        assert!(err.contains("not found"));
    }
}
