use std::path::{Path, PathBuf};

use crate::plan::Plan;

// bodies into prompts — the registry stays a pointer, content is fetched on demand.
pub const PLAN_SKILL_REGISTRY_POINTER: &str =
    "Plan skills registry: `codeterm plan skill list` to browse, `codeterm plan skill show <slug>` to read one — never inline skill bodies.";

// Cap the slug list rendered into the single-line heartbeat block.
const REGISTRY_LINE_MAX_SLUGS: usize = 8;

fn plan_skill_state_dir(plan: &Plan) -> Option<PathBuf> {
    if plan.id.trim().is_empty() {
        return None;
    }
    Some(super::paths::plan_state_dir_unchecked(
        super::paths::plan_sidecar_root(plan),
        &plan.status,
        &plan.id,
    ))
}

pub fn count_plan_skills_for(plan: &Plan) -> usize {
    plan_skill_state_dir(plan)
        .map(|dir| count_plan_skills(&dir))
        .unwrap_or(0)
}

pub fn list_plan_skill_slugs(plan: &Plan) -> Vec<String> {
    let Some(skills_dir) = plan_skill_state_dir(plan).map(|dir| dir.join("skills")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
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

/// Fill the pinned `PLAN_SKILLS_REGISTRY_LINE` template — single source of the
/// wording lives in `prompts.rs`; here we only substitute count + slug region.
pub fn render_plan_skills_registry_line(count: usize, slugs: &[String]) -> String {
    let list = if slugs.is_empty() {
        "none yet".to_string()
    } else {
        let mut shown = slugs
            .iter()
            .take(REGISTRY_LINE_MAX_SLUGS)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if slugs.len() > REGISTRY_LINE_MAX_SLUGS {
            shown.push_str(", …");
        }
        shown
    };
    super::prompts_plan::PLAN_SKILLS_REGISTRY_LINE
        .replace("{n}", &count.to_string())
        .replace("{slug1}, {slug2}, …", &list)
}

pub fn render_plan_skill_registry_line(plan_id: &str, count: usize) -> String {
    if count == 0 {
        return format!(
            "Plan skills (plan {plan_id}): none registered — capture a reusable procedure with `codeterm plan skill new <slug>`."
        );
    }
    let noun = if count == 1 { "skill" } else { "skills" };
    format!(
        "Plan skills (plan {plan_id}): {count} {noun} registered — `codeterm plan skill list` to browse, `codeterm plan skill show <slug>` to read (bodies never inlined)."
    )
}

pub fn count_plan_skills(plan_dir: &Path) -> usize {
    let skills_dir = plan_dir.join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_count_points_to_new_command() {
        let line = render_plan_skill_registry_line("plan_abc", 0);
        assert!(line.contains("plan_abc"));
        assert!(line.contains("none registered"));
        assert!(line.contains("codeterm plan skill new"));
    }

    #[test]
    fn nonzero_line_is_a_pointer_not_content() {
        let line = render_plan_skill_registry_line("plan_abc", 3);
        assert!(line.contains("3 skills registered"));
        assert!(line.contains("codeterm plan skill list"));
        assert!(line.contains("never inlined"));
        // A pointer line stays short; it must never carry SKILL.md bodies.
        assert!(line.len() < 200);
    }

    #[test]
    fn singular_noun_for_one_skill() {
        let line = render_plan_skill_registry_line("p1", 1);
        assert!(line.contains("1 skill registered"));
        assert!(!line.contains("1 skills"));
    }

    #[test]
    fn pointer_constant_carries_no_body() {
        assert!(PLAN_SKILL_REGISTRY_POINTER.contains("codeterm plan skill list"));
        assert!(PLAN_SKILL_REGISTRY_POINTER.len() < 200);
    }

    #[test]
    fn count_only_dirs_with_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let plan_dir = tmp.path();
        let skills = plan_dir.join("skills");
        std::fs::create_dir_all(skills.join("alpha")).unwrap();
        std::fs::write(skills.join("alpha").join("SKILL.md"), "# Alpha").unwrap();
        std::fs::create_dir_all(skills.join("beta")).unwrap();
        std::fs::write(skills.join("beta").join("SKILL.md"), "# Beta").unwrap();
        // Directory without SKILL.md is ignored.
        std::fs::create_dir_all(skills.join("stray")).unwrap();
        // A stray ledger file is not a skill.
        std::fs::write(skills.join("ledger.jsonl"), "{}").unwrap();
        assert_eq!(count_plan_skills(plan_dir), 2);
    }

    #[test]
    fn count_missing_skills_dir_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(count_plan_skills(tmp.path()), 0);
    }

    #[test]
    fn registry_line_empty_reads_none_yet() {
        let line = render_plan_skills_registry_line(0, &[]);
        assert!(line.contains("Plan skills (0): none yet"));
        assert!(line.contains("codeterm plan skill show <slug> | new | edit."));
    }

    #[test]
    fn registry_line_lists_slugs_and_count() {
        let slugs = vec!["seed-fixtures".to_string(), "run-eval".to_string()];
        let line = render_plan_skills_registry_line(slugs.len(), &slugs);
        assert!(line.contains("Plan skills (2): seed-fixtures, run-eval —"));
        assert!(!line.contains("{n}"));
        assert!(!line.contains("{slug1}"));
    }

    #[test]
    fn registry_line_truncates_long_slug_lists() {
        let slugs: Vec<String> = (0..12).map(|i| format!("s{i}")).collect();
        let line = render_plan_skills_registry_line(slugs.len(), &slugs);
        assert!(line.contains("Plan skills (12):"));
        assert!(line.contains(", …"));
        assert!(!line.contains("s11"));
    }
}
