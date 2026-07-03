use std::path::Path;

// Shared one-line pointer for heartbeat/worker charter. Never inline skill
// bodies into prompts — the registry stays a pointer, content is fetched on demand.
pub const PLAN_SKILL_REGISTRY_POINTER: &str =
    "Plan skills registry: `codeterm plan skill list` to browse, `codeterm plan skill show <slug>` to read one — never inline skill bodies.";

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
}
