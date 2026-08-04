//! Hidden graph-worker that extracts entities/relations from mem_items.
//! Also hosts the per-turn evolution observer scaffold (Track 8 of v1.1.23,
//! issue #58) — bumps `evidence_count` on skill/lesson mem_items when the
//! next observation is success-shaped. Auto-retire / rank formula land in
//! v1.1.24; this is foundation only.

use std::collections::HashMap;

use parking_lot::Mutex;
use rusqlite::Connection;

use codeterm_types::config::MemoryConfig;

use crate::ports::MemHostContext;
use crate::store::MemKind;
use crate::MemManager;

fn should_extract(kind: MemKind) -> bool {
    !matches!(kind, MemKind::Summary)
}

pub fn should_extract_public(kind: MemKind) -> bool {
    should_extract(kind)
}

pub fn build_extraction_prompt(
    host: &dyn MemHostContext,
    mem_cfg: &MemoryConfig,
    mem_item_id: &str,
    title: &str,
    body: &str,
) -> String {
    let truncated = if body.len() > 50_000 {
        let mut end = 50_000;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        &body[..end]
    } else {
        body
    };
    let areas_block = format_existing_areas(host);
    let graph_types_block = format_existing_graph_types(host);
    let dedup_threshold = mem_cfg.dedup_threshold;
    crate::prompt_text::MEM_EXTRACTION
        .replace("{id}", mem_item_id)
        .replace("{title}", title)
        .replace("{body}", truncated)
        .replace("{existing_areas}", &areas_block)
        .replace("{existing_graph_types}", &graph_types_block)
        .replace("{dedup_threshold}", &format!("{:.2}", dedup_threshold))
}

// Feed existing taxonomy into prompts so workers pick known mem areas by usage order.
// Prompt only suggests existing areas; only main agent can create new ones.
fn format_existing_areas(host: &dyn MemHostContext) -> String {
    let tax = match host.get_taxonomy(None) {
        Ok(t) => t,
        Err(_) => return "(none yet — use \"\" if unsure)".to_string(),
    };
    let mut rows: Vec<(String, u64)> = tax
        .into_iter()
        .filter(|(area, _)| area != "(none)" && !area.is_empty())
        .map(|(area, by_kind)| {
            let total: u64 = by_kind.values().sum();
            (area, total)
        })
        .collect();
    if rows.is_empty() {
        return "(none yet — use \"\" if unsure)".to_string();
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows.truncate(12);
    rows.into_iter()
        .map(|(a, n)| format!("- {} ({} items)", a, n))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_existing_graph_types(host: &dyn MemHostContext) -> String {
    let (entity_types, relation_types) = host.query_graph_type_counts();

    let mut out = String::new();

    if entity_types.is_empty() {
        out.push_str("Entity types: (none yet — common types: Person, Project, Tech, Machine)\n");
    } else {
        out.push_str("Entity types in use:\n");
        for (t, n) in &entity_types {
            out.push_str(&format!("- {} ({} entities)\n", t, n));
        }
    }

    if relation_types.is_empty() {
        out.push_str("Relation types: (none yet — use verb-based: works_on, uses, shipped, runs_on, mentions)");
    } else {
        out.push_str("Relation types in use:\n");
        for (t, n) in &relation_types {
            out.push_str(&format!("- {} ({} relations)\n", t, n));
        }
    }

    out
}

/// Find mem_items that are eligible for extraction but have no child items
/// linked back via `extracted_from`. These are either (a) never extracted,
/// or (b) extractions that produced zero child items. Caller decides.
pub fn find_pending_items(
    mem: &MemManager,
    limit: usize,
) -> Result<Vec<(String, String, String, MemKind)>, String> {
    use crate::store::MemKind::*;
    mem.with_conn(|conn| {
        // worker has already run against it — we don't need to re-queue.
        let mut stmt = conn
            .prepare(
                r#"SELECT m.id, m.kind, m.title, COALESCE(m.body_inline, '')
               FROM mem_items m
               WHERE m.kind IN ('conversation','note','daily','learning','fact')
                 AND m.tags_json NOT LIKE '%auto-extracted%'
                 AND NOT EXISTS (
                   SELECT 1 FROM relations r
                   WHERE r.from_id = m.id OR r.to_id = m.id
                 )
               ORDER BY m.created_at DESC
               LIMIT ?1"#,
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([limit as i64], |r| {
                let id: String = r.get(0)?;
                let kind_str: String = r.get(1)?;
                let title: String = r.get(2)?;
                let body: String = r.get(3)?;
                let kind = match kind_str.as_str() {
                    "conversation" => Conversation,
                    "note" => Note,
                    "daily" => Daily,
                    "learning" => Learning,
                    "fact" => Fact,
                    _ => Note,
                };
                Ok((id, title, body, kind))
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

// Track injected `kind=skill` and `kind=lesson` items so we can bump evidence on matching output.
// Only successful-shaped observations count; raw echoes and failed/error-looking outputs are ignored.

/// Tracked context entry per scoped key (e.g. tab id, worker id).
#[derive(Clone, Debug)]
pub struct InjectedItem {
    pub mem_id: String,
    pub kind: MemKind,
    /// First ~512 chars of the body, lowercased. Used to detect when a
    /// later observation is just quoting the skill back at us.
    pub body_signature: String,
}

impl InjectedItem {
    pub fn new(mem_id: String, kind: MemKind, body: &str) -> Self {
        let mut sig: String = body.chars().take(512).collect();
        sig.make_ascii_lowercase();
        Self {
            mem_id,
            kind,
            body_signature: sig,
        }
    }
}

/// In-memory ring of tracked injections keyed by scope (tab / worker).
/// One entry per scope holds the most-recent batch of skill/lesson ids.
/// Cleared when an observation is consumed against it.
pub struct EvolutionObserver {
    by_scope: Mutex<HashMap<String, Vec<InjectedItem>>>,
}

impl Default for EvolutionObserver {
    fn default() -> Self {
        Self {
            by_scope: Mutex::new(HashMap::new()),
        }
    }
}

impl EvolutionObserver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `items` were injected into `scope`'s context. Replaces any
    /// previous batch for the scope — only the most-recent set is in flight.
    pub fn record_injection(&self, scope: &str, items: Vec<InjectedItem>) {
        let mut by_scope = self.by_scope.lock();
        if items.is_empty() {
            by_scope.remove(scope);
        } else {
            by_scope.insert(scope.to_string(), items);
        }
    }

    /// Drain the tracked items for `scope` and apply the observation.
    /// Returns the list of mem_ids that got `evidence_count` bumped.
    /// `observation` is the raw text of the next agent output / tool result.
    pub fn observe(&self, conn: &Connection, scope: &str, observation: &str) -> Vec<String> {
        let items = {
            let mut by_scope = self.by_scope.lock();
            by_scope.remove(scope).unwrap_or_default()
        };
        if items.is_empty() {
            return Vec::new();
        }
        if !is_success_shaped(observation) {
            return Vec::new();
        }
        let obs_lower = observation.to_ascii_lowercase();
        let mut bumped = Vec::new();
        for item in items {
            if observation_echoes_body(&obs_lower, &item.body_signature) {
                continue;
            }
            match codeterm_mem_store::graph::bump_evidence_count(conn, &item.mem_id) {
                Ok(true) => bumped.push(item.mem_id),
                Ok(false) => {
                    log::debug!(
                        "[evolution-observer] mem_id {} no longer present, skipping bump",
                        item.mem_id
                    );
                }
                Err(e) => {
                    log::warn!(
                        "[evolution-observer] bump failed for {}: {}",
                        item.mem_id,
                        e
                    );
                }
            }
        }
        bumped
    }

    #[cfg(test)]
    fn pending(&self, scope: &str) -> Vec<InjectedItem> {
        self.by_scope.lock().get(scope).cloned().unwrap_or_default()
    }
}

/// Crude success heuristic for v1.1.23: no explicit error markers.
/// v1.1.24 will tighten this with exit-code awareness + corrective-tool
/// detection.
fn is_success_shaped(observation: &str) -> bool {
    let lower = observation.to_ascii_lowercase();
    !(lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || lower.contains("traceback"))
}

/// False-corroboration mitigation: if the observation is largely a quote
/// of the injected body it doesn't count as independent evidence. We
/// require at least 80 contiguous chars of overlap from the signature.
fn observation_echoes_body(observation_lower: &str, body_sig_lower: &str) -> bool {
    if body_sig_lower.len() < 80 {
        return false;
    }
    let probe_len = body_sig_lower.len().min(120);
    let probe = &body_sig_lower[..probe_len];
    observation_lower.contains(probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn open_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::init_schema(&conn).unwrap();
        conn
    }

    fn insert_skill(conn: &Connection, id: &str, body: &str) {
        conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, tags_json, created_at, updated_at)
             VALUES (?1, ?1, 'skill', 'global', 't', ?2, '[]', '2026-01-01', '2026-01-01')",
            rusqlite::params![id, body],
        ).unwrap();
    }

    fn evidence_count(conn: &Connection, id: &str) -> i64 {
        conn.query_row(
            "SELECT evidence_count FROM mem_items WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn observer_bumps_on_success_shaped_observation() {
        let conn = open_test_conn();
        insert_skill(&conn, "skill_a", "follow these steps to refresh tokens");
        let obs = EvolutionObserver::new();
        obs.record_injection(
            "tab1",
            vec![InjectedItem::new(
                "skill_a".into(),
                MemKind::Skill,
                "follow these steps to refresh tokens",
            )],
        );
        let bumped = obs.observe(
            &conn,
            "tab1",
            "Done. Token refreshed and request returned 200 OK.",
        );
        assert_eq!(bumped, vec!["skill_a".to_string()]);
        assert_eq!(evidence_count(&conn, "skill_a"), 2);
        assert!(
            obs.pending("tab1").is_empty(),
            "scope cleared after observe"
        );
    }

    #[test]
    fn observer_skips_when_observation_carries_error_marker() {
        let conn = open_test_conn();
        insert_skill(&conn, "skill_b", "follow these steps");
        let obs = EvolutionObserver::new();
        obs.record_injection(
            "tab1",
            vec![InjectedItem::new(
                "skill_b".into(),
                MemKind::Skill,
                "follow these steps",
            )],
        );
        let bumped = obs.observe(&conn, "tab1", "Error: command failed with exit code 1");
        assert!(bumped.is_empty());
        assert_eq!(
            evidence_count(&conn, "skill_b"),
            1,
            "no bump on error-shaped observation"
        );
    }

    #[test]
    fn observer_skips_when_observation_echoes_skill_body() {
        let conn = open_test_conn();
        let body = "When refreshing JWT tokens always include the kid header read from the JWKS endpoint, otherwise Auth0 returns 401 with realm=api.example.com";
        insert_skill(&conn, "skill_c", body);
        let obs = EvolutionObserver::new();
        obs.record_injection(
            "tab1",
            vec![InjectedItem::new("skill_c".into(), MemKind::Skill, body)],
        );
        let echo = format!(
            "Working on it. Following the rule: {}. Should succeed shortly.",
            body
        );
        let bumped = obs.observe(&conn, "tab1", &echo);
        assert!(
            bumped.is_empty(),
            "echo of body must not count as independent evidence"
        );
        assert_eq!(evidence_count(&conn, "skill_c"), 1);
    }

    #[test]
    fn observer_no_op_when_nothing_tracked() {
        let conn = open_test_conn();
        let obs = EvolutionObserver::new();
        let bumped = obs.observe(&conn, "tab1", "ok");
        assert!(bumped.is_empty());
    }

    #[test]
    fn observer_replaces_prior_batch_for_same_scope() {
        let conn = open_test_conn();
        insert_skill(&conn, "skill_x", "x body");
        insert_skill(&conn, "skill_y", "y body");
        let obs = EvolutionObserver::new();
        obs.record_injection(
            "tab1",
            vec![InjectedItem::new(
                "skill_x".into(),
                MemKind::Skill,
                "x body",
            )],
        );
        obs.record_injection(
            "tab1",
            vec![InjectedItem::new(
                "skill_y".into(),
                MemKind::Skill,
                "y body",
            )],
        );
        let bumped = obs.observe(&conn, "tab1", "ok");
        assert_eq!(bumped, vec!["skill_y".to_string()]);
        assert_eq!(
            evidence_count(&conn, "skill_x"),
            1,
            "x not in tracked batch when observe ran"
        );
        assert_eq!(evidence_count(&conn, "skill_y"), 2);
    }

    #[test]
    fn observer_handles_missing_row_gracefully() {
        let conn = open_test_conn();
        let obs = EvolutionObserver::new();
        obs.record_injection(
            "tab1",
            vec![InjectedItem::new(
                "ghost".into(),
                MemKind::Skill,
                "vanished",
            )],
        );
        let bumped = obs.observe(&conn, "tab1", "ok");
        assert!(bumped.is_empty(), "missing row → no bump reported");
    }

    #[test]
    fn is_success_shaped_recognizes_common_failure_words() {
        assert!(is_success_shaped("Done. exit code 0."));
        assert!(!is_success_shaped("ERROR: something blew up"));
        assert!(!is_success_shaped("the build failed"));
        assert!(!is_success_shaped("thread 'main' panicked"));
        assert!(!is_success_shaped("Traceback (most recent call last):"));
    }
}
