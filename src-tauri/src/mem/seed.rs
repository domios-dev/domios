//! Seed bundled starter mem_items on first install. Idempotent via marker file.
//! Bodies live as .md files under `src-tauri/prompts/seed/` and are compiled
//! into the binary via `include_str!`, matching the pattern used by all other
//! prompt templates.

use super::{MemKind, MemScope, NewMemItem, MemManager};

const SEED_MARKER: &str = ".bundled-seeded-v1";

const IDENTITY_BODY: &str = include_str!("../../prompts/seed/identity_placeholder.md");
const RULE_NEVER_EDIT_AUTOGEN: &str = include_str!("../../prompts/seed/rule_never_edit_autogen.md");
const RULE_SAVE_TO_MEM: &str = include_str!("../../prompts/seed/rule_save_to_mem.md");
const RULE_SEARCH_BEFORE_ASKING: &str = include_str!("../../prompts/seed/rule_search_before_asking.md");
const SKILL_CODETERM: &str = include_str!("../../prompts/seed/skill_codeterm.md");

pub fn seed_bundled(mem: &MemManager) -> Result<usize, String> {
    let marker = mem.db_path().parent().unwrap_or(std::path::Path::new(".")).join(SEED_MARKER);
    if marker.exists() {
        return Ok(0);
    }

    let items: Vec<NewMemItem> = vec![
        NewMemItem {
            kind: MemKind::Preference,
            scope: MemScope::Global,
            title: "identity".into(),
            summary: Some("Agent identity placeholder — overwrite via mem save".into()),
            area: None,
            tags: vec!["bundled".into(), "identity".into()],
            body: IDENTITY_BODY.trim().into(),
            source: Some("bundled:v1".into()),
            importance: 5.0,
        },
        NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "never edit autogen files".into(),
            summary: Some("CLAUDE.md / AGENTS.md / AGENT.md are auto-regenerated".into()),
            area: None,
            tags: vec!["bundled".into(), "autogen".into()],
            body: RULE_NEVER_EDIT_AUTOGEN.trim().into(),
            source: Some("bundled:v1".into()),
            importance: 5.0,
        },
        NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "save to mem not provider memory".into(),
            summary: Some("Persistent memory lives in mem only".into()),
            area: None,
            tags: vec!["bundled".into(), "memory".into()],
            body: RULE_SAVE_TO_MEM.trim().into(),
            source: Some("bundled:v1".into()),
            importance: 5.0,
        },
        NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "search before asking".into(),
            summary: Some("Query mem before asking the user".into()),
            area: None,
            tags: vec!["bundled".into(), "workflow".into()],
            body: RULE_SEARCH_BEFORE_ASKING.trim().into(),
            source: Some("bundled:v1".into()),
            importance: 5.0,
        },
        NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "codeterm".into(),
            summary: Some("CodeTerm CLI skill — pane / workspace / agent / mem control".into()),
            area: Some("codeterm".into()),
            tags: vec!["bundled".into(), "core".into(), "cli".into()],
            body: SKILL_CODETERM.trim().into(),
            source: Some("bundled:v1".into()),
            importance: 4.0,
        },
    ];

    let mut count = 0;
    for item in items {
        if mem.save(item).is_ok() {
            count += 1;
        }
    }

    let _ = std::fs::write(&marker, b"v1\n");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_runs_once() {
        let mem = MemManager::open_in_memory().unwrap();
        // The in-memory DB sets db_path to a tmp dir without marker
        let n1 = seed_bundled(&mem).unwrap();
        assert!(n1 >= 5, "expected ≥5 seeded items, got {}", n1);
        // Re-running should be a no-op (marker file written)
        let n2 = seed_bundled(&mem).unwrap();
        assert_eq!(n2, 0, "second seed should be no-op");
    }

    #[test]
    fn seeded_identity_appears_in_wake_up() {
        let mem = MemManager::open_in_memory().unwrap();
        seed_bundled(&mem).unwrap();
        let out = mem.wake_up(&super::super::WakeUpOptions::default()).unwrap();
        assert!(out.contains("L0 — Identity"));
        assert!(out.contains("L1 — Critical context"));
        assert!(out.contains("never edit autogen") || out.contains("Rules"));
    }
}
