//! Cron task-brief composition — one builder shared by the scheduler's
//! automatic fire and the manual `cron_trigger` path so they can't drift
//! apart (they did: `cron_trigger` silently dropped the skill block).

/// `skill_block` is pre-resolved by the caller (`cron::skill_block::cron_skill_block`)
/// so this stays pure — no filesystem access, fully deterministic to test.
pub fn build_cron_task_text(
    task: &str,
    skill_block: Option<&str>,
    report_target_tab_id: Option<&str>,
    run_id: &str,
) -> String {
    let mut task_text = task.to_string();
    if let Some(block) = skill_block {
        task_text.push_str(block);
    }
    if report_target_tab_id.is_some() {
        task_text.push_str(crate::prompts::CRON_REPORT_HINT);
    }
    task_text.push_str(&crate::prompts::CRON_COMPLETE_SUFFIX.replace("{}", run_id));
    task_text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::skill_block::cron_skill_block;

    #[test]
    fn resolved_skill_path_reaches_the_spawned_agent_spawn_spec_task() {
        // Nonexistent slug -> cron_skill_block resolves the real MISSING branch,
        // deterministic without touching a real skill folder.
        let block = cron_skill_block("nonexistent-drill-slug");
        let task = build_cron_task_text("opaque-cron-task", Some(&block), None, "run-1");
        assert!(task.contains("nonexistent-drill-slug"));
        assert!(task.contains("SKILL.md"));
        assert!(task.contains("opaque-cron-task"));
        assert!(task.contains("run-1"));
    }

    #[test]
    fn no_skill_declared_means_no_skill_block_in_the_spawned_task() {
        let task = build_cron_task_text("opaque-cron-task", None, None, "opaque-run");
        let suffix = crate::prompts::CRON_COMPLETE_SUFFIX.replace("{}", "opaque-run");
        assert_eq!(task, format!("opaque-cron-task{suffix}"));
    }

    #[test]
    fn report_hint_only_appended_when_report_target_is_present() {
        let hint = crate::prompts::CRON_REPORT_HINT;
        let with_target =
            build_cron_task_text("opaque-task", None, Some("opaque-tab"), "opaque-run");
        let without_target = build_cron_task_text("opaque-task", None, None, "opaque-run");
        assert!(with_target.contains(hint));
        assert!(!without_target.contains(hint));
    }
}
