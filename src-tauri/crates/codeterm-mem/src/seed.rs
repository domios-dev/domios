//! Seed bundled starter mem_items on first install. Idempotent via marker file.
//! Bodies live as .md files under `src-tauri/prompts/seed/` and are compiled
//! into the binary via `include_str!`, matching the pattern used by all other
//! prompt templates.

use super::{store::MemUpdate, MemKind, MemManager, MemScope, NewMemItem, SearchQuery};

const SEED_MARKER: &str = ".bundled-seeded-v2";

const IDENTITY_BODY: &str = include_str!("../../../prompts/seed/identity_placeholder.md");
const RULE_NEVER_EDIT_AUTOGEN: &str =
    include_str!("../../../prompts/seed/rule_never_edit_autogen.md");
const RULE_SAVE_TO_MEM: &str = include_str!("../../../prompts/seed/rule_save_to_mem.md");
const RULE_SEARCH_BEFORE_ASKING: &str =
    include_str!("../../../prompts/seed/rule_search_before_asking.md");
const SKILL_CODETERM: &str = include_str!("../../../prompts/seed/skill_codeterm.md");
const LEGACY_SKILL_CODETERM: &str = r#"## CodeTerm CLI

Use `codeterm` to control tabs, workspaces, agents, mesh peers, mem, and system power.

- `codeterm tab list` — always use this plain compact view for interactive discovery/cleanup; `(self)` marks your tab
- `codeterm tab inspect <id>` — full details for one selected tab; never write or run whole-list JSON interactively
- `codeterm send "..." --tab ID` — type into a tab
- `codeterm tab stop [--tab ID]` — interrupt the agent's current generation (doesn't close the tab)
- `codeterm agent spawn claude --task "..."` — spawn a child tab you own (parent = you, automatic)
- `codeterm mesh peers` — list mesh peers
- `codeterm mem search/get/save/wake-up` — unified memory (search runs FTS + embeddings hybrid in one call)
- `codeterm mem worker spawn --intent "enrich" --task "..."` — spawn a headless graph worker for custom enrichment
- `codeterm mem sync` — trigger librarian file sync for source-tracked items
- `codeterm notify "Done" --level info` — system / in-app notification
- `codeterm keepawake start --duration-secs N [--display]` / `stop` / `status` — prevent sleep cross-platform (do NOT call `caffeinate`, `powercfg`, `systemd-inhibit`)

Always use the CLI from inside CodeTerm via Bash. Do not curl the REST API directly."#;
const LEGACY_SKILL_SUMMARY: &str =
    "CodeTerm CLI skill — tab / workspace / agent / mem control";
const LEGACY_SKILL_TAGS: &[&str] = &["bundled", "core", "cli"];

fn is_exact_legacy_codeterm(item: &super::store::MemItem) -> bool {
    item.kind == "skill"
        && item.scope == "global"
        && item.title == "codeterm"
        && item.summary.as_deref() == Some(LEGACY_SKILL_SUMMARY)
        && item.area.as_deref() == Some("codeterm")
        && item.tags.iter().map(String::as_str).eq(LEGACY_SKILL_TAGS.iter().copied())
        && item.body == LEGACY_SKILL_CODETERM.trim()
        && item.source.as_deref() == Some("bundled:v1")
        && item.importance == 4.0
}

pub fn seed_bundled(mem: &MemManager) -> Result<usize, String> {
    let marker = mem
        .db_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(SEED_MARKER);
    let items: Vec<NewMemItem> = vec![
        NewMemItem {
            kind: MemKind::Preference,
            scope: MemScope::Global,
            title: "identity".into(),
            summary: Some("Agent identity placeholder — overwrite via mem save".into()),
            area: None,
            tags: vec!["bundled".into(), "identity".into()],
            body: IDENTITY_BODY.trim().into(),
            source: Some("bundled:v2".into()),
            importance: 5.0,
            source_path: None,
            pinned: false,
        },
        NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "never edit autogen files".into(),
            summary: Some("CLAUDE.md / AGENTS.md / AGENT.md are auto-regenerated".into()),
            area: None,
            tags: vec!["bundled".into(), "autogen".into()],
            body: RULE_NEVER_EDIT_AUTOGEN.trim().into(),
            source: Some("bundled:v2".into()),
            importance: 5.0,
            source_path: None,
            pinned: false,
        },
        NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "save to mem not provider memory".into(),
            summary: Some("Persistent memory lives in mem only".into()),
            area: None,
            tags: vec!["bundled".into(), "memory".into()],
            body: RULE_SAVE_TO_MEM.trim().into(),
            source: Some("bundled:v2".into()),
            importance: 5.0,
            source_path: None,
            pinned: false,
        },
        NewMemItem {
            kind: MemKind::Rule,
            scope: MemScope::Global,
            title: "search before asking".into(),
            summary: Some("Query mem before asking the user".into()),
            area: None,
            tags: vec!["bundled".into(), "workflow".into()],
            body: RULE_SEARCH_BEFORE_ASKING.trim().into(),
            source: Some("bundled:v2".into()),
            importance: 5.0,
            source_path: None,
            pinned: false,
        },
        NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "codeterm".into(),
            summary: Some("CodeTerm CLI skill — tab / workspace / agent / mem control".into()),
            area: Some("codeterm".into()),
            tags: vec!["bundled".into(), "core".into(), "cli".into()],
            body: SKILL_CODETERM.trim().into(),
            source: Some("bundled:v2".into()),
            importance: 4.0,
            source_path: None,
            pinned: false,
        },
    ];

    let mut count = 0;
    for item in items {
        let existing = mem
            .search(&SearchQuery {
                query: item.title.clone(),
                kind: Some(item.kind.as_str().into()),
                limit: Some(20),
                ..Default::default()
            })
            .unwrap_or_default()
            .into_iter()
            .find(|hit| hit.item.title == item.title);
        if let Some(hit) = existing {
            let stale_bundled = is_exact_legacy_codeterm(&hit.item);
            if stale_bundled
                && mem
                    .update(
                        &hit.item.id,
                        MemUpdate {
                            body: Some(item.body.clone()),
                            summary: item.summary.clone(),
                            tags: Some(item.tags.clone()),
                            ..Default::default()
                        },
                    )
                    .is_ok()
            {
                count += 1;
            }
        } else if mem.save(item).is_ok() {
            count += 1;
        }
    }

    let _ = std::fs::write(&marker, b"v2\n");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_skill(body: &str) -> NewMemItem {
        NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "codeterm".into(),
            summary: Some(LEGACY_SKILL_SUMMARY.into()),
            area: Some("codeterm".into()),
            tags: LEGACY_SKILL_TAGS.iter().map(|tag| (*tag).into()).collect(),
            body: body.into(),
            source: Some("bundled:v1".into()),
            importance: 4.0,
            source_path: None,
            pinned: false,
        }
    }

    #[test]
    fn seed_runs_once() {
        let mem = MemManager::open_in_memory().unwrap();
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
        let out = mem.wake_up(&crate::WakeUpOptions::default()).unwrap();
        assert!(out.contains("L0 — Identity"));
        assert!(out.contains("L1 — Critical context"));
        assert!(out.contains("never edit autogen") || out.contains("Rules"));
    }

    #[test]
    fn bundled_skill_refreshes_stale_seed_without_overwriting_user_memory() {
        let stale = MemManager::open_in_memory().unwrap();
        stale.save(legacy_skill(LEGACY_SKILL_CODETERM)).unwrap();
        seed_bundled(&stale).unwrap();
        let refreshed = stale
            .search(&SearchQuery {
                query: "codeterm".into(),
                kind: Some("skill".into()),
                limit: Some(5),
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|hit| hit.item.title == "codeterm")
            .unwrap();
        assert_eq!(refreshed.item.body, SKILL_CODETERM.trim());
        assert_ne!(refreshed.item.body, LEGACY_SKILL_CODETERM.trim());
        assert!(refreshed.item.body.contains("CODETERM_TAB_ID"));

        let edited = MemManager::open_in_memory().unwrap();
        let user_body = format!("{LEGACY_SKILL_CODETERM}\nuser-authored edit");
        edited.save(legacy_skill(&user_body)).unwrap();
        seed_bundled(&edited).unwrap();
        let preserved = edited
            .search(&SearchQuery {
                query: "codeterm".into(),
                kind: Some("skill".into()),
                limit: Some(5),
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .find(|hit| hit.item.title == "codeterm")
            .unwrap();
        assert_eq!(preserved.item.body, user_body);

        let mutations: [fn(&mut NewMemItem); 3] = [
            |item: &mut NewMemItem| item.summary = Some("user summary".into()),
            |item: &mut NewMemItem| item.tags.push("user-tag".into()),
            |item: &mut NewMemItem| item.importance = 4.5,
        ];
        for mutate in mutations {
            let mem = MemManager::open_in_memory().unwrap();
            let mut record = legacy_skill(LEGACY_SKILL_CODETERM);
            mutate(&mut record);
            let id = mem.save(record).unwrap();
            let mut before = mem.get(&id).unwrap().unwrap();
            seed_bundled(&mem).unwrap();
            let mut after = mem.get(&id).unwrap().unwrap();
            before.accessed_at = None;
            before.access_count = 0;
            after.accessed_at = None;
            after.access_count = 0;
            let before = serde_json::to_vec(&before).unwrap();
            let after = serde_json::to_vec(&after).unwrap();
            assert_eq!(after, before, "edited legacy metadata must remain byte-exact");
        }
    }
}
