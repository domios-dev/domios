use super::*;
use crate::mem::store::{MemKind, MemScope, NewMemItem};
use crate::mem::MemManager;
use chrono::Timelike;
use rusqlite::{params, Connection};

#[tokio::test]
async fn light_phase_promotes_accessed_learnings() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save(NewMemItem {
            kind: MemKind::Learning,
            scope: MemScope::Global,
            title: "test".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();

    {
        let conn = mem.conn();
        let conn = conn.lock();
        let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
        conn.execute(
            "UPDATE mem_items SET created_at = ?1, accessed_at = ?2, access_count = 3 WHERE id = ?3",
            params![&two_days_ago, &two_days_ago, &id],
        )
        .unwrap();
    }

    let report = light_phase(&mem).await.unwrap();
    assert_eq!(report.considered, 1);
    assert_eq!(report.promoted, 1);

    let after = mem.get(&id).unwrap().unwrap();
    assert_eq!(after.kind, "fact");
    assert!(after.importance >= 4.0);
}

#[tokio::test]
async fn deep_phase_expires_old_unaccessed_conversations() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save(NewMemItem {
            kind: MemKind::Conversation,
            scope: MemScope::Global,
            title: "old chat".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "lorem ipsum".into(),
            source: None,
            importance: 2.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();

    // Backdate to 60 days ago, never accessed
    {
        let conn = mem.conn();
        let conn = conn.lock();
        let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        conn.execute(
            "UPDATE mem_items SET created_at = ?1, access_count = 0 WHERE id = ?2",
            params![&sixty_days_ago, &id],
        )
        .unwrap();
    }

    let report = deep_phase(&mem).await.unwrap();
    assert_eq!(report.dropped, 1);
    // Item still exists but with importance = 0 (expired, not deleted)
    let after = mem.get(&id).unwrap().unwrap();
    assert_eq!(after.importance, 0.0);
}

#[test]
fn ensure_dreaming_cron_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let mut store = crate::cron::store::CronStore::new_with_path(path);

    let first = ensure_dreaming_cron(&mut store).unwrap();
    assert!(first, "first call should create");
    let second = ensure_dreaming_cron(&mut store).unwrap();
    assert!(!second, "second call should be no-op");
    let count = store
        .list()
        .iter()
        .filter(|j| j.name == DREAMING_CRON_NAME)
        .count();
    assert_eq!(count, 1, "exactly one auto-cron must exist");

    let job = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap();
    assert_eq!(
        job.jitter_max_ms,
        Some(1_800_000),
        "jitter must be 0..30 min delay"
    );
    assert!(job.enabled);
    assert_eq!(job.schedule, "0 6 * * *");
}

#[test]
fn legacy_jitter_90min_migrates_to_30min() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let mut store = crate::cron::store::CronStore::new_with_path(path);

    ensure_dreaming_cron(&mut store).unwrap();
    let id = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap()
        .id
        .clone();
    if let Some(j) = store.get_mut(&id) {
        j.jitter_max_ms = Some(5_400_000);
    }
    let _ = store.save();

    ensure_dreaming_cron(&mut store).unwrap();

    let job = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap();
    assert_eq!(
        job.jitter_max_ms,
        Some(1_800_000),
        "legacy 90min must be auto-migrated to 30min"
    );
}

#[test]
fn legacy_3am_schedule_migrates_to_6am() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let mut store = crate::cron::store::CronStore::new_with_path(path);

    ensure_dreaming_cron(&mut store).unwrap();
    let id = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap()
        .id
        .clone();
    if let Some(j) = store.get_mut(&id) {
        j.schedule = "0 3 * * *".into();
    }
    let _ = store.save();

    ensure_dreaming_cron(&mut store).unwrap();

    let job = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap();
    assert_eq!(
        job.schedule, "0 6 * * *",
        "legacy 3am must be auto-migrated to 6am"
    );
}

#[test]
fn dreaming_telegram_step_substitution() {
    // Enabled (default): the placeholder must expand to the real bridge-send instructions so the dreamer pushes its 🧠 brief to Telegram.
    let enabled = crate::prompts::DREAMING_REM.replace(
        "{telegram_notify_step}",
        super::DREAMING_TELEGRAM_STEP_ENABLED,
    );
    assert!(
        enabled.contains("codeterm bridge send"),
        "enabled prompt must instruct bridge send"
    );
    assert!(
        enabled.contains("🧠 Dreamer brief"),
        "enabled prompt must keep the brief tag"
    );
    assert!(
        !enabled.contains("{telegram_notify_step}"),
        "placeholder must be substituted, not literal"
    );

    // Disabled: the placeholder expands to an explicit "skip" directive and the bridge-send command must NOT appear in the final prompt.
    let disabled = crate::prompts::DREAMING_REM.replace(
        "{telegram_notify_step}",
        super::DREAMING_TELEGRAM_STEP_DISABLED,
    );
    assert!(
        !disabled.contains("codeterm bridge send"),
        "disabled prompt must NOT mention bridge send anywhere"
    );
    assert!(
        disabled.contains("DISABLED"),
        "disabled prompt must explicitly flag opt-out"
    );
    assert!(
        disabled.contains("dreaming_telegram_notify"),
        "disabled prompt must cite the config key so user knows what to flip"
    );
}

#[test]
fn legacy_schedule_migration_recomputes_next_run() {
    // Issue #96: migrating "0 3 * * *" -> "0 6 * * *" must also reset next_run, else the on-disk value points at the OLD schedule's window and the
    // scheduler fires a recovery run on startup.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let mut store = crate::cron::store::CronStore::new_with_path(path);

    ensure_dreaming_cron(&mut store).unwrap();
    let id = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap()
        .id
        .clone();

    // Simulate legacy state: stale next_run from the old "0 3 * * *" cadence.
    let stale_legacy_next =
        crate::cron::store::compute_next_run_jittered_in("0 3 * * *", Some(5_400_000), None)
            .unwrap()
            .unwrap();
    if let Some(j) = store.get_mut(&id) {
        j.schedule = "0 3 * * *".into();
        j.jitter_max_ms = Some(5_400_000);
        j.next_run = Some(stale_legacy_next);
    }
    let _ = store.save();

    ensure_dreaming_cron(&mut store).unwrap();

    let job = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap();
    assert_eq!(job.schedule, "0 6 * * *");
    assert_eq!(job.jitter_max_ms, Some(1_800_000));
    // next_run must have moved off the legacy schedule.
    let new_next = job.next_run.expect("next_run must be set after migration");
    assert_ne!(
        new_next, stale_legacy_next,
        "migration must recompute next_run, not leave it anchored to the old schedule",
    );
    // And the new value must fit the new "0 6 * * *" cadence: minute-of-hour=0 plus 0..30min jitter window relative to UTC.
    let new_dt = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(new_next as i64).unwrap();
    let minute = new_dt.minute();
    assert!(
        minute <= 30,
        "post-migration next_run must land in the 06:00..06:30 jitter window, got minute={minute}",
    );
}

#[test]
fn custom_user_schedule_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let mut store = crate::cron::store::CronStore::new_with_path(path);

    ensure_dreaming_cron(&mut store).unwrap();
    let id = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap()
        .id
        .clone();
    if let Some(j) = store.get_mut(&id) {
        j.schedule = "30 12 * * *".into();
    }
    let _ = store.save();

    ensure_dreaming_cron(&mut store).unwrap();

    let job = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap();
    assert_eq!(
        job.schedule, "30 12 * * *",
        "user-customised schedule must NOT be overwritten"
    );
}

#[test]
fn dream_cron_enabled_state_round_trips_through_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let mut store = crate::cron::store::CronStore::new_with_path(path);

    ensure_dreaming_cron(&mut store).expect("create dream cron");
    let id = store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
        .unwrap()
        .id
        .clone();

    let enabled_now = |store: &crate::cron::store::CronStore, id: &str| -> bool {
        store.list().iter().find(|j| j.id == id).unwrap().enabled
    };

    assert!(
        enabled_now(&store, &id),
        "fresh dreaming cron must be enabled"
    );

    store.set_enabled(&id, false).unwrap();
    assert!(
        !enabled_now(&store, &id),
        "disable must persist through store API"
    );

    store.set_enabled(&id, true).unwrap();
    assert!(
        enabled_now(&store, &id),
        "re-enable must persist through store API"
    );
}

fn insert_entity(conn: &Connection, id: &str, etype: &str, age_days: i64) {
    let ts = (chrono::Utc::now() - chrono::Duration::days(age_days)).to_rfc3339();
    crate::mem::graph::insert_entity(
        conn,
        &crate::mem::graph::Entity {
            id: id.into(),
            entity_type: etype.into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    // Backdate so orphan-cleanup tests have an aged entity.
    conn.execute(
        "UPDATE entities SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![&ts, id],
    )
    .unwrap();
}

fn insert_relation(conn: &Connection, from: &str, rel: &str, to: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    crate::mem::graph::insert_relation(
        conn,
        &crate::mem::graph::Relation {
            from_id: from.into(),
            relation_type: rel.into(),
            to_id: to.into(),
            scope: "global".into(),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        },
    )
    .unwrap();
}

#[tokio::test]
async fn normalize_merges_high_confidence_asymmetric_pair() {
    // Typo-class pair: robert vs robertt. LD=1, ratio=6/7=0.857 ≥ 0.85. robertt has 1 relation, robert has 3 → robertt becomes alias of robert.
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        for id in ["person_robert", "person_robertt", "x", "y", "z"] {
            let etype = if id.starts_with("person") {
                "person"
            } else {
                "thing"
            };
            insert_entity(&c, id, etype, 0);
        }
        insert_relation(&c, "person_robertt", "mentions", "x");
        for to in ["x", "y", "z"] {
            insert_relation(&c, "person_robert", "mentions", to);
        }
    }

    let report = normalize_phase(&mem).await.unwrap();
    assert_eq!(report.promoted, 1, "exactly one merge expected");

    let canonical: String = mem
        .conn()
        .lock()
        .query_row(
            "SELECT canonical_id FROM entity_aliases WHERE alias_id = 'person_robertt'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(canonical, "person_robert");
}

#[tokio::test]
async fn normalize_ignores_nickname_pair_below_flag_threshold() {
    // Honest documentation: Levenshtein cannot catch nickname↔fullname (tima vs timofey). Ratio ≈ 0.43 < 0.70 → ignored entirely.
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_entity(&c, "person_tima", "person", 0);
        insert_entity(&c, "person_timofey", "person", 0);
    }
    normalize_phase(&mem).await.unwrap();
    let aliases: i64 = mem
        .conn()
        .lock()
        .query_row("SELECT COUNT(*) FROM entity_aliases", [], |r| r.get(0))
        .unwrap();
    assert_eq!(aliases, 0, "nickname-class pair must not be auto-merged");
}

#[tokio::test]
async fn normalize_canonicalizes_synonymous_relations() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_entity(&c, "e_a", "thing", 0);
        insert_entity(&c, "e_b", "thing", 0);
        insert_relation(&c, "e_a", "references", "e_b");
    }

    normalize_phase(&mem).await.unwrap();
    let kind: String = mem
        .conn()
        .lock()
        .query_row(
            "SELECT relation_type FROM relations WHERE from_id='e_a' AND to_id='e_b'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind, "mentions");
}

#[tokio::test]
async fn normalize_deletes_old_orphan_entities() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_entity(&c, "orphan_1", "person", 20);
        insert_entity(&c, "kind_protected", "kind", 20);
    }

    normalize_phase(&mem).await.unwrap();
    let exists_orphan: i64 = mem
        .conn()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id='orphan_1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let exists_kind: i64 = mem
        .conn()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id='kind_protected'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exists_orphan, 0, "orphan must be deleted");
    assert_eq!(exists_kind, 1, "protected entity_type must survive");
}

#[tokio::test]
async fn normalize_invalidates_broken_relations() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_entity(&c, "e_a", "thing", 0);
        // Relation pointing to non-existent entity 'ghost'.
        insert_relation(&c, "e_a", "rel", "ghost");
    }

    normalize_phase(&mem).await.unwrap();
    let valid_to: Option<String> = mem
        .conn()
        .lock()
        .query_row(
            "SELECT valid_to FROM relations WHERE from_id='e_a' AND to_id='ghost'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(valid_to.is_some(), "broken relation must be invalidated");
}

#[tokio::test]
async fn normalize_preserves_relations_anchored_in_mem_items() {
    let mem = MemManager::open_in_memory().unwrap();
    let fact_id = mem
        .save(NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "rust borrow checker".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_entity(&c, "tech_rust", "tech", 0);
        insert_relation(&c, &fact_id, "mentions", "tech_rust");
    }

    normalize_phase(&mem).await.unwrap();
    let valid_to: Option<String> = mem
        .conn()
        .lock()
        .query_row(
            "SELECT valid_to FROM relations WHERE to_id='tech_rust'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        valid_to.is_none(),
        "fact_*→entity relation must survive normalize"
    );
}

#[tokio::test]
async fn normalize_preserves_mem_item_to_mem_item_relations() {
    let mem = MemManager::open_in_memory().unwrap();
    let parent = mem
        .save(NewMemItem {
            kind: MemKind::Conversation,
            scope: MemScope::Global,
            title: "parent".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    let child = mem
        .save(NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "child".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_relation(&c, &child, "extracted_from", &parent);
    }

    normalize_phase(&mem).await.unwrap();
    let valid_to: Option<String> = mem
        .conn()
        .lock()
        .query_row(
            "SELECT valid_to FROM relations WHERE from_id=?1 AND to_id=?2",
            params![&child, &parent],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        valid_to.is_none(),
        "mem_item→mem_item relation must survive normalize"
    );
}

#[tokio::test]
async fn normalize_flags_borderline_pair_without_merging() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn();
        let c = c.lock();
        insert_entity(&c, "person_anya", "person", 0);
        insert_entity(&c, "person_anna", "person", 0);
    }

    normalize_phase(&mem).await.unwrap();
    let aliases: i64 = mem
        .conn()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM entity_aliases WHERE alias_id IN ('person_anya','person_anna')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        aliases, 0,
        "borderline pair (ratio < 0.92) must not be auto-merged"
    );
}

#[tokio::test]
async fn deep_phase_keeps_high_importance_conversations() {
    let mem = MemManager::open_in_memory().unwrap();
    let id = mem
        .save(NewMemItem {
            kind: MemKind::Conversation,
            scope: MemScope::Global,
            title: "important chat".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 4.5,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    {
        let conn = mem.conn();
        let conn = conn.lock();
        let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        conn.execute(
            "UPDATE mem_items SET created_at = ?1 WHERE id = ?2",
            params![&sixty_days_ago, &id],
        )
        .unwrap();
    }

    let r = deep_phase(&mem).await.unwrap();
    assert_eq!(r.dropped, 0);
    assert!(mem.get(&id).unwrap().is_some());
}
