//! Cron task-brief composition — one builder shared by the scheduler's
//! automatic fire and the manual `cron_trigger` path so they can't drift
//! apart (they did: `cron_trigger` silently dropped the skill block).

/// `skill_block` is pre-resolved by the caller (`commands::cron_skill_block`)
/// so this stays pure — no filesystem access, fully deterministic to test.
pub fn build_cron_task_text(
    task: &str,
    skill_block: Option<&str>,
    creator_tab_id: Option<&str>,
    run_id: &str,
) -> String {
    let mut task_text = task.to_string();
    if let Some(block) = skill_block {
        task_text.push_str(block);
    }
    if creator_tab_id.is_some() {
        task_text.push_str(crate::prompts::CRON_REPORT_HINT);
    }
    task_text.push_str(&crate::prompts::CRON_COMPLETE_SUFFIX.replace("{}", run_id));
    task_text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::cron_skill_block;
    use crate::service::spawn::AgentSpawnSpec;

    #[test]
    fn resolved_skill_path_reaches_the_spawned_agent_spawn_spec_task() {
        // Nonexistent slug -> cron_skill_block resolves the real MISSING branch,
        // deterministic without touching a real skill folder.
        let block = cron_skill_block("nonexistent-drill-slug");
        let task_text = build_cron_task_text("do the thing", Some(&block), None, "run-1");

        let spec = AgentSpawnSpec {
            task: Some(task_text),
            ..Default::default()
        };

        let task = spec.task.expect("task must be set");
        assert!(task.contains("nonexistent-drill-slug"));
        assert!(task.to_lowercase().contains("cron complete"));
        assert!(task.contains("cron complete --run-id run-1"));
    }

    #[test]
    fn no_skill_declared_means_no_skill_block_in_the_spawned_task() {
        let task_text = build_cron_task_text("do the thing", None, None, "run-1");
        let spec = AgentSpawnSpec {
            task: Some(task_text),
            ..Default::default()
        };
        let task = spec.task.unwrap();
        assert!(!task.contains("SKILL AVAILABLE"));
        assert!(!task.contains("SKILL MISSING"));
        assert!(task.contains("cron complete --run-id run-1"));
    }

    #[test]
    fn report_hint_only_appended_when_creator_tab_id_present() {
        let with_creator = build_cron_task_text("t", None, Some("tab-1"), "r1");
        let without_creator = build_cron_task_text("t", None, None, "r1");
        assert!(with_creator.contains("report progress"));
        assert!(!without_creator.contains("report progress"));
    }
}
