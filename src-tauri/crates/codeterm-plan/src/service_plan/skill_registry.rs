use std::path::{Path, PathBuf};

use crate::plan::Plan;

// bodies into prompts — the registry stays a pointer, content is fetched on demand.
pub const PLAN_SKILL_REGISTRY_POINTER: &str =
    "Plan skills registry: `codeterm plan skill list` to browse, `codeterm plan skill show <slug>` to read one — never inline skill bodies.";

// Cap the skill summaries rendered into the single-line heartbeat block.
const REGISTRY_LINE_MAX_SLUGS: usize = 8;
const REGISTRY_LINE_MAX_SLUG_CHARS: usize = 32;
const REGISTRY_LINE_MAX_PURPOSE_CHARS: usize = 56;

// `pub`, not `pub(super)`: the app crate's heartbeat-delivery tests assert
// against this structured summary directly rather than parsing rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSkillRegistrySummary {
    pub slug: String,
    pub purpose: Option<String>,
}

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
    list_plan_skill_summaries(plan)
        .into_iter()
        .map(|skill| skill.slug)
        .collect()
}

pub fn list_plan_skill_summaries(plan: &Plan) -> Vec<PlanSkillRegistrySummary> {
    super::plan_skills::plan_skill_manifest_for_plan(plan)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| PlanSkillRegistrySummary {
            slug: entry.slug,
            purpose: (!entry.purpose.is_empty()).then_some(entry.purpose),
        })
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

/// Fill the pinned `PLAN_SKILLS_REGISTRY_LINE` template — single source of the
/// wording lives in `prompts.rs`; here we only substitute count + summary region.
pub(super) fn render_plan_skills_registry_line(
    count: usize,
    skills: &[PlanSkillRegistrySummary],
) -> String {
    let list = if count == 0 {
        "none yet".to_string()
    } else if skills.is_empty() {
        "skill metadata unavailable".to_string()
    } else {
        let shown_count = skills.len().min(REGISTRY_LINE_MAX_SLUGS);
        let mut shown = skills
            .iter()
            .take(REGISTRY_LINE_MAX_SLUGS)
            .map(|skill| {
                let slug = truncate_chars(skill.slug.trim(), REGISTRY_LINE_MAX_SLUG_CHARS);
                let purpose = skill
                    .purpose
                    .as_deref()
                    .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|value| !value.is_empty())
                    .map(|value| truncate_chars(&value, REGISTRY_LINE_MAX_PURPOSE_CHARS))
                    .unwrap_or_else(|| "description unavailable".to_string());
                format!("{slug}: {purpose}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        if count > shown_count {
            shown.push_str(&format!("; … +{} more", count - shown_count));
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
    super::plan_skills::plan_skill_manifest_from_dir(plan_dir)
        .map(|manifest| manifest.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::OrchestratorRef;

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
        std::fs::create_dir_all(skills.join(".pending")).unwrap();
        std::fs::write(skills.join(".pending").join("SKILL.md"), "# Pending").unwrap();
        // Directory without SKILL.md is ignored.
        std::fs::create_dir_all(skills.join("stray")).unwrap();
        std::fs::write(
            skills.join("ledger.jsonl"),
            "{\"seq\":1,\"slug\":\"alpha\",\"verdict\":\"keep\",\"rationale\":\"fixture\"}\n{\"seq\":2,\"slug\":\"beta\",\"verdict\":\"keep\",\"rationale\":\"fixture\"}\n",
        )
        .unwrap();
        assert_eq!(count_plan_skills(plan_dir), 2);
    }

    #[test]
    fn count_missing_skills_dir_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(count_plan_skills(tmp.path()), 0);
    }

    #[test]
    fn registry_resolves_active_plan_skills_from_external_state_root() {
        let project = tempfile::tempdir().unwrap();
        let plan = Plan::new(
            "External registry".to_string(),
            project.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        let expected = super::super::paths::global_plan_database_path()
            .parent()
            .unwrap()
            .join("plans")
            .join("active")
            .join(&plan.id);
        assert_eq!(
            plan_skill_state_dir(&plan).as_deref(),
            Some(expected.as_path())
        );
        assert!(!expected.starts_with(project.path()));
    }

    #[test]
    fn registry_line_zero_is_stable() {
        assert_eq!(
            render_plan_skills_registry_line(0, &[]),
            "Plan skills (0): none yet — codeterm plan skill show <slug> | new | edit."
        );
    }

    #[test]
    fn registry_line_lists_one_skill_with_its_persisted_purpose() {
        let skills = vec![PlanSkillRegistrySummary {
            slug: "seed-fixtures".to_string(),
            purpose: Some("Seed deterministic fixtures.".to_string()),
        }];
        assert_eq!(
            render_plan_skills_registry_line(skills.len(), &skills),
            "Plan skills (1): seed-fixtures: Seed deterministic fixtures. — codeterm plan skill show <slug> | new | edit."
        );
    }

    #[test]
    fn registry_line_lists_multiple_skills_and_marks_missing_descriptions() {
        let skills = vec![
            PlanSkillRegistrySummary {
                slug: "run-eval".to_string(),
                purpose: Some("Compare prompt variants.".to_string()),
            },
            PlanSkillRegistrySummary {
                slug: "seed-fixtures".to_string(),
                purpose: None,
            },
        ];
        assert_eq!(
            render_plan_skills_registry_line(skills.len(), &skills),
            "Plan skills (2): run-eval: Compare prompt variants.; seed-fixtures: description unavailable — codeterm plan skill show <slug> | new | edit."
        );
    }

    #[test]
    fn registry_line_truncates_descriptions_and_skill_count_deterministically() {
        let skills: Vec<PlanSkillRegistrySummary> = (0..10)
            .map(|i| PlanSkillRegistrySummary {
                slug: format!("skill-{i}"),
                purpose: Some("x".repeat(100)),
            })
            .collect();
        let line = render_plan_skills_registry_line(skills.len(), &skills);
        assert!(line.starts_with("Plan skills (10): skill-0: "));
        assert!(line.contains("…; skill-7: "));
        assert!(line.contains("; … +2 more — codeterm plan skill show <slug> | new | edit."));
        assert!(!line.contains("skill-8:"));
        assert!(!line.contains('\n'));
        assert!(line.chars().count() < 800);
    }
}
