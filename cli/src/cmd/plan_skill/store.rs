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

// Resolve a plan sidecar directory. Precedence: explicit --plan id ->
// $CODETERM_PLAN_ID -> the single active plan on disk. The env var is only a
// convenience shortcut, never a hard dependency: worker panes without it still
// resolve via the on-disk single-active-plan fallback. Ambiguous (0 or >1
// active plans, no id) is an explicit error asking for --plan.
pub fn resolve_plan_dir(
    project_root: &Path,
    plan_id: Option<String>,
) -> Result<(String, PathBuf), String> {
    let plans = plans_dir(project_root);
    let requested = plan_id
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("CODETERM_PLAN_ID").ok().filter(|s| !s.trim().is_empty()));

    if let Some(id) = requested {
        let files = plan_files(&plans, Some(&id))?;
        let path = files.into_iter().next().ok_or_else(|| {
            format!("plan {id} not found under {}", plans.display())
        })?;
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
            let plan_json = entry.path().join("plan.json");
            if plan_json.is_file() {
                candidates.push(plan_json);
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
            let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
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

pub fn scaffold(slug: &str, title: &str, area: &str, check: Option<&str>) -> String {
    let check_fm = match check {
        Some(cmd) if !cmd.trim().is_empty() => format!("check: {}\n", cmd.trim()),
        _ => String::new(),
    };
    let check_section = match check {
        Some(cmd) if !cmd.trim().is_empty() => {
            format!("\n## Check\n\nVerify with: `{}`\n", cmd.trim())
        }
        _ => String::new(),
    };
    format!(
        "---\ntitle: {title}\narea: {area}\ntags: [plan-skill]\n{check_fm}---\n\n# {title}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…{check_section}\n\n## Scripts\n\n- `./scripts/{slug}.sh` — describe what it does\n",
    )
}

pub fn list_skills(plan_dir: &Path) -> Vec<String> {
    let root = skills_root(plan_dir);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut slugs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    slugs.sort();
    slugs
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

    #[test]
    fn scaffold_has_frontmatter_and_sections() {
        let md = scaffold("deploy", "Deploy", "infra", None);
        assert!(md.starts_with("---\n"));
        assert!(md.contains("title: Deploy"));
        assert!(md.contains("area: infra"));
        assert!(md.contains("tags: [plan-skill]"));
        assert!(md.contains("## When to use"));
        assert!(md.contains("./scripts/deploy.sh"));
        assert!(!md.contains("check:"));
        assert!(!md.contains("## Check"));
    }

    #[test]
    fn scaffold_persists_check_metadata() {
        let md = scaffold("deploy", "Deploy", "infra", Some("./build.sh check"));
        assert!(md.contains("check: ./build.sh check"));
        assert!(md.contains("## Check"));
        assert!(md.contains("`./build.sh check`"));
    }

    fn write_plan(plans: &Path, bucket: &str, id: &str) -> PathBuf {
        let dir = plans.join(bucket).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plan.json"),
            serde_json::json!({ "id": id }).to_string(),
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
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let plans = plans_dir(root);
        write_plan(&plans, "active", "solo");
        // Env must not leak from the test runner.
        std::env::remove_var("CODETERM_PLAN_ID");
        let (id, _dir) = resolve_plan_dir(root, None).unwrap();
        assert_eq!(id, "solo");
    }

    #[test]
    fn resolve_ambiguous_active_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let plans = plans_dir(root);
        write_plan(&plans, "active", "a");
        write_plan(&plans, "active", "b");
        std::env::remove_var("CODETERM_PLAN_ID");
        let err = resolve_plan_dir(root, None).unwrap_err();
        assert!(err.contains("disambiguate"));
    }

    #[test]
    fn resolve_missing_plan_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let err = resolve_plan_dir(root, Some("ghost".into())).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn list_skills_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let plan_dir = tmp.path();
        for slug in ["zebra", "alpha"] {
            let d = skill_dir(plan_dir, slug);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), "# x").unwrap();
        }
        assert_eq!(list_skills(plan_dir), vec!["alpha", "zebra"]);
    }
}
