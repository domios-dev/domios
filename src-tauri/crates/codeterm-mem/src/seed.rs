//! Seed bundled starter mem_items on first install, and refresh them on
//! every startup when their bundled content changes. Bodies live as .md
//! files under `src-tauri/prompts/seed/`, compiled in via `include_str!`.

use sha2::{Digest, Sha256};

use super::{MemKind, MemManager, MemScope, NewMemItem, SearchQuery};

const SEED_MARKER: &str = ".bundled-seeded-v2";

const IDENTITY_BODY: &str = include_str!("../../../prompts/seed/identity_placeholder.md");
const RULE_NEVER_EDIT_AUTOGEN: &str =
    include_str!("../../../prompts/seed/rule_never_edit_autogen.md");
const RULE_SAVE_TO_MEM: &str = include_str!("../../../prompts/seed/rule_save_to_mem.md");
const RULE_SEARCH_BEFORE_ASKING: &str =
    include_str!("../../../prompts/seed/rule_search_before_asking.md");
const SKILL_CODETERM: &str = include_str!("../../../prompts/seed/skill_codeterm.md");

// Frozen 1.10.7 (`e07b04be8`) shipped bodies — a bootstrap bridge for the
// real installed base seeded before fingerprinting existed. Never edited.
const LEGACY_1_10_7_IDENTITY_BODY: &str =
    include_str!("../../../prompts/seed/legacy_1_10_7/identity_placeholder.md");
const LEGACY_1_10_7_RULE_NEVER_EDIT_AUTOGEN: &str =
    include_str!("../../../prompts/seed/legacy_1_10_7/rule_never_edit_autogen.md");
const LEGACY_1_10_7_RULE_SAVE_TO_MEM: &str =
    include_str!("../../../prompts/seed/legacy_1_10_7/rule_save_to_mem.md");
const LEGACY_1_10_7_RULE_SEARCH_BEFORE_ASKING: &str =
    include_str!("../../../prompts/seed/legacy_1_10_7/rule_search_before_asking.md");
const LEGACY_1_10_7_SKILL_CODETERM: &str =
    include_str!("../../../prompts/seed/legacy_1_10_7/skill_codeterm.md");

/// A JSON array (never a joined/delimited string) of every field the
/// seeder manages; `source` is the provenance marker and is excluded.
fn seed_fingerprint(item: &NewMemItem) -> String {
    let NewMemItem {
        kind,
        scope,
        title,
        summary,
        area,
        tags,
        body,
        source: _source,
        importance,
        source_path,
        pinned,
    } = item;
    let canonical = serde_json::json!([
        kind.as_str(),
        scope.as_str(),
        title,
        summary,
        area,
        tags,
        body,
        importance.to_bits(),
        source_path,
        pinned,
    ]);
    let bytes = serde_json::to_vec(&canonical).expect("array of primitives always serializes");
    format!("{:x}", Sha256::digest(&bytes))
}

/// Why a bundled item's managed fields were left untouched this pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedSkipReason {
    /// `managed_payload` could not be read back (e.g. an unrecognized kind).
    Unreadable,
    /// No fingerprint is recorded, and the stored payload matches neither
    /// today's shipped payload nor the frozen 1.10.7 one.
    Absent,
    /// A fingerprint is recorded but no longer matches the stored payload —
    /// something changed it since the last bootstrap or refresh.
    Mismatched,
    /// The write lost a race: another writer changed the fingerprint
    /// between this pass reading it and attempting the refresh.
    ConcurrentEdit,
}

/// What one `seed_bundled` pass did, itemized rather than a single count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedOutcome {
    pub created: usize,
    pub bootstrapped: usize,
    pub refreshed: usize,
    pub skipped: Vec<(String, SeedSkipReason)>,
}

impl SeedOutcome {
    pub fn written(&self) -> usize {
        self.created + self.bootstrapped + self.refreshed
    }
}

/// The exact `1.10.7` (`e07b04be8`) shipped payload — frozen historical
/// data, NOT derived from `bundled_items()` so it can't drift with it.
fn legacy_1_10_7_items() -> Vec<NewMemItem> {
    vec![
        NewMemItem {
            kind: MemKind::Preference,
            scope: MemScope::Global,
            title: "identity".into(),
            summary: Some("Agent identity placeholder — overwrite via mem save".into()),
            area: None,
            tags: vec!["bundled".into(), "identity".into()],
            body: LEGACY_1_10_7_IDENTITY_BODY.trim().into(),
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
            body: LEGACY_1_10_7_RULE_NEVER_EDIT_AUTOGEN.trim().into(),
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
            body: LEGACY_1_10_7_RULE_SAVE_TO_MEM.trim().into(),
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
            body: LEGACY_1_10_7_RULE_SEARCH_BEFORE_ASKING.trim().into(),
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
            body: LEGACY_1_10_7_SKILL_CODETERM.trim().into(),
            source: Some("bundled:v2".into()),
            importance: 4.0,
            source_path: None,
            pinned: false,
        },
    ]
}

fn bundled_items() -> Vec<NewMemItem> {
    vec![
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
    ]
}

pub fn seed_bundled(mem: &MemManager) -> Result<SeedOutcome, String> {
    seed_from(mem, bundled_items())
}

/// Testable core: takes the shipped item list explicitly so tests can drive
/// the identical mechanism against v2/v3/v4-style fixtures.
fn seed_from(mem: &MemManager, items: Vec<NewMemItem>) -> Result<SeedOutcome, String> {
    let marker = mem
        .db_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(SEED_MARKER);
    let mut outcome = SeedOutcome::default();

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

        let Some(hit) = existing else {
            // Payload and fingerprint land in the SAME insert — never a
            // two-step create-then-bootstrap with an unfingerprinted window.
            if mem.save_seeded(item.clone(), &seed_fingerprint(&item)).is_ok() {
                outcome.created += 1;
            }
            continue;
        };

        let (current, recorded_fingerprint) = match mem.managed_payload(&hit.item.id) {
            Ok(Some(pair)) => pair,
            Ok(None) => continue,
            Err(_) => {
                outcome
                    .skipped
                    .push((item.title.clone(), SeedSkipReason::Unreadable));
                continue;
            }
        };

        match recorded_fingerprint {
            Some(recorded) => {
                if seed_fingerprint(&current) != recorded {
                    outcome
                        .skipped
                        .push((item.title.clone(), SeedSkipReason::Mismatched));
                    continue;
                }
                if current != item {
                    apply_refresh(mem, &hit.item.id, &recorded, &item, &mut outcome);
                }
            }
            None => {
                // Exactly today's shipped payload, or exactly the frozen
                // 1.10.7 one — either is proof of "still unedited".
                let bootstrap_fingerprint = if current == item {
                    Some(seed_fingerprint(&item))
                } else {
                    legacy_1_10_7_items()
                        .into_iter()
                        .find(|legacy| legacy.title == item.title && current == *legacy)
                        .map(|legacy| seed_fingerprint(&legacy))
                };
                match bootstrap_fingerprint {
                    Some(fp) => match mem.bootstrap_seed_fingerprint(&hit.item.id, &current, &fp) {
                        Ok(true) => {
                            outcome.bootstrapped += 1;
                            // Same pass, chained: its own separately gated
                            // CAS, never a blind write off the bootstrap.
                            if current != item {
                                apply_refresh(mem, &hit.item.id, &fp, &item, &mut outcome);
                            }
                        }
                        Ok(false) => outcome
                            .skipped
                            .push((item.title.clone(), SeedSkipReason::ConcurrentEdit)),
                        Err(_) => outcome
                            .skipped
                            .push((item.title.clone(), SeedSkipReason::Unreadable)),
                    },
                    None => outcome
                        .skipped
                        .push((item.title.clone(), SeedSkipReason::Absent)),
                }
            }
        }
    }

    let _ = std::fs::write(&marker, b"v2\n");
    Ok(outcome)
}

/// Refresh to `item`'s current payload, guarded by its own CAS — never a
/// blind write, even when chained right after a bootstrap just proved it.
fn apply_refresh(
    mem: &MemManager,
    id: &str,
    expected_fingerprint: &str,
    item: &NewMemItem,
    outcome: &mut SeedOutcome,
) {
    let new_fingerprint = seed_fingerprint(item);
    match mem.cas_refresh_bundled(id, Some(expected_fingerprint), item, &new_fingerprint) {
        Ok(true) => outcome.refreshed += 1,
        Ok(false) => outcome
            .skipped
            .push((item.title.clone(), SeedSkipReason::ConcurrentEdit)),
        Err(_) => outcome
            .skipped
            .push((item.title.clone(), SeedSkipReason::Unreadable)),
    }
}

#[cfg(test)]
mod tests;
