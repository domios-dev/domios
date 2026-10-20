/// Task-brief block for a cron job's `--skill <slug>`: the exact `SKILL.md`
/// path to Read, resolved against the canonical dir (never provider dirs).
pub fn cron_skill_block(slug: &str) -> String {
    cron_skill_block_at(slug, &crate::paths::data_dir().join("skills"))
}

fn cron_skill_block_at(slug: &str, skills_root: &std::path::Path) -> String {
    let skill_md = skills_root.join(slug).join("SKILL.md");
    let path_str = skill_md.display().to_string();
    if skill_md.exists() {
        crate::prompts::CRON_SKILL_HINT
            .replace("{skill_slug}", slug)
            .replace("{skill_path}", &path_str)
    } else {
        crate::prompts::CRON_SKILL_MISSING
            .replace("{skill_slug}", slug)
            .replace("{skill_path}", &path_str)
    }
}

#[cfg(test)]
mod skill_block_tests {
    use super::cron_skill_block_at;

    #[test]
    fn found_skill_points_at_exact_path_and_forbids_skill_tool() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("scratch-drill-skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "# Scratch drill\nBody.").unwrap();

        let block = cron_skill_block_at("scratch-drill-skill", tmp.path());
        assert!(block.contains("scratch-drill-skill"));
        assert!(block.contains("SKILL.md"));
        assert!(block.to_lowercase().contains("do not use the skill tool"));
        assert!(!block.to_lowercase().contains("find "));
    }

    #[test]
    fn missing_skill_tells_agent_to_report_not_search() {
        let tmp = tempfile::tempdir().unwrap();
        let block = cron_skill_block_at("nonexistent-slug", tmp.path());
        assert!(block.contains("nonexistent-slug"));
        assert!(block.to_lowercase().contains("cron complete"));
        assert!(block.to_lowercase().contains("find"));
        assert!(block.to_lowercase().contains("never"));
    }
}
