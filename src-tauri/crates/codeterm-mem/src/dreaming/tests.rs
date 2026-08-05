use super::*;
use crate::store::{MemKind, MemScope, NewMemItem};
use crate::MemManager;
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
fn dreaming_telegram_step_substitution() {
    // Enabled (default): the placeholder must expand to the real bridge-send instructions so the dreamer pushes its 🧠 brief to Telegram.
    let enabled = crate::prompt_text::DREAMING_REM.replace(
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
    let disabled = crate::prompt_text::DREAMING_REM.replace(
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

fn insert_entity(conn: &Connection, id: &str, etype: &str, age_days: i64) {
    let ts = (chrono::Utc::now() - chrono::Duration::days(age_days)).to_rfc3339();
    crate::graph::insert_entity(
        conn,
        &crate::graph::Entity {
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
    crate::graph::insert_relation(
        conn,
        &crate::graph::Relation {
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

// --- ensure_dreaming_cron: task command + self-heal ---

mod cron_task {
    use crate::ports::{MemCronCreateParams, MemCronJob, MemSchedulerPort};
    use crate::worker_agent::BackgroundModelPort;
    use codeterm_types::config::MemoryConfig;

    struct FakeCompat;
    impl BackgroundModelPort for FakeCompat {
        fn resolve(&self, _provider: &str, _configured: &str) -> (String, bool) {
            (String::new(), false)
        }
    }

    #[derive(Default)]
    struct MockScheduler {
        jobs: Vec<MemCronJob>,
        created: Vec<MemCronCreateParams>,
    }

    impl MemSchedulerPort for MockScheduler {
        fn list_jobs(&self) -> Vec<MemCronJob> {
            self.jobs.clone()
        }
        fn find_job_by_name(&self, name: &str) -> Option<MemCronJob> {
            self.jobs.iter().find(|j| j.name == name).cloned()
        }
        fn update_job(
            &mut self,
            id: &str,
            update: &mut dyn FnMut(&mut MemCronJob),
        ) -> Result<(), String> {
            let job = self
                .jobs
                .iter_mut()
                .find(|j| j.id == id)
                .ok_or_else(|| format!("not found: {id}"))?;
            update(job);
            Ok(())
        }
        fn set_job_protected(&mut self, _id: &str, _protected: bool) -> Result<(), String> {
            Ok(())
        }
        fn create_job(&mut self, params: MemCronCreateParams) -> Result<(), String> {
            self.jobs.push(MemCronJob {
                id: format!("job_{}", self.jobs.len()),
                name: params.name.clone(),
                agent: params.agent.clone().unwrap_or_default(),
                schedule: params.schedule.clone(),
                task: params.task.clone(),
                enabled: params.enabled.unwrap_or(true),
                oneshot: params.oneshot.unwrap_or(false),
                next_run: None,
                jitter_max_ms: params.jitter_max_ms,
                tz: params.tz.clone(),
                protected: false,
            });
            self.created.push(params);
            Ok(())
        }
        fn delete_job(&mut self, id: &str) -> Result<(), String> {
            self.jobs.retain(|j| j.id != id);
            Ok(())
        }
        fn save(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn compute_next_run_jittered_in(
            &self,
            _schedule: &str,
            _jitter: Option<u64>,
            _tz: Option<&str>,
        ) -> Result<Option<u64>, String> {
            Ok(Some(1))
        }
    }

    fn ensure(store: &mut MockScheduler) -> Result<bool, String> {
        crate::dreaming::ensure_dreaming_cron(store, &MemoryConfig::default(), "claude", &FakeCompat)
    }

    #[test]
    fn fresh_task_is_runnable_dream_run() {
        let mut store = MockScheduler::default();
        assert!(ensure(&mut store).unwrap(), "first call creates");
        let task = &store.created[0].task;
        assert!(
            task.contains("codeterm mem dream run"),
            "task must invoke the `run` subcommand, got: {task:?}"
        );
        assert!(
            !task.contains("mem dream &&"),
            "task must not use the bare (help-only) `mem dream` form: {task:?}"
        );
    }

    #[test]
    fn self_heals_stale_bare_dream_task() {
        let mut store = MockScheduler::default();
        ensure(&mut store).unwrap();
        // Simulate an existing install left with the old broken command.
        let id = store.jobs[0].id.clone();
        store.jobs[0].task =
            "codeterm mem dream && codeterm agent report \"dreaming complete\"\n".into();

        let created = ensure(&mut store).unwrap();
        assert!(!created, "second call must not create a duplicate");
        assert!(
            store.jobs[0].task.contains("codeterm mem dream run"),
            "stale task must be healed to `mem dream run`, got: {:?}",
            store.jobs[0].task
        );
        assert_eq!(id, store.jobs[0].id, "self-heal must edit in place, not recreate");
    }

    #[test]
    fn healed_task_is_idempotent() {
        let mut store = MockScheduler::default();
        ensure(&mut store).unwrap();
        let healed = store.jobs[0].task.clone();
        ensure(&mut store).unwrap();
        assert_eq!(healed, store.jobs[0].task, "no churn once task is correct");
    }
}
