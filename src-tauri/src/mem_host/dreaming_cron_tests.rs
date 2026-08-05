#[cfg(test)]
mod tests {
    use chrono::Timelike;
    use codeterm_types::config::MemoryConfig;

    use crate::cron::store::CronStore;
    use crate::mem::dreaming::{ensure_dreaming_cron, DREAMING_CRON_NAME};
    use crate::mem_host::CronSchedulerPort;

    fn test_mem_cfg() -> MemoryConfig {
        MemoryConfig::default()
    }

    fn ensure_dreaming(store: &mut CronStore) -> Result<bool, String> {
        ensure_dreaming_cron(
            &mut CronSchedulerPort(store),
            &test_mem_cfg(),
            "claude",
            &crate::service::spawn::CatalogBackgroundModelPort,
        )
    }

    #[test]
    fn ensure_dreaming_cron_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = CronStore::new_with_path(path);

        let first = ensure_dreaming(&mut store).unwrap();
        assert!(first, "first call should create");
        let second = ensure_dreaming(&mut store).unwrap();
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
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).unwrap();
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

        ensure_dreaming(&mut store).unwrap();

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
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).unwrap();
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

        ensure_dreaming(&mut store).unwrap();

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
    fn legacy_schedule_migration_recomputes_next_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).unwrap();
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();

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

        ensure_dreaming(&mut store).unwrap();

        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap();
        assert_eq!(job.schedule, "0 6 * * *");
        assert_eq!(job.jitter_max_ms, Some(1_800_000));
        let new_next = job.next_run.expect("next_run must be set after migration");
        assert_ne!(
            new_next, stale_legacy_next,
            "migration must recompute next_run, not leave it anchored to the old schedule",
        );
        let new_dt =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(new_next as i64).unwrap();
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
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).unwrap();
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

        ensure_dreaming(&mut store).unwrap();

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
    fn fresh_dream_task_uses_runnable_run_subcommand() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).unwrap();
        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .clone();
        assert!(
            job.task.contains("codeterm mem dream run"),
            "task must invoke `mem dream run`, got: {:?}",
            job.task
        );
    }

    #[test]
    fn stale_bare_dream_task_self_heals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).unwrap();
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();
        if let Some(j) = store.get_mut(&id) {
            j.task = "codeterm mem dream && codeterm agent report \"dreaming complete\"\n".into();
        }
        let _ = store.save();

        let created = ensure_dreaming(&mut store).unwrap();
        assert!(!created, "self-heal must not create a duplicate");
        let job = store.list().iter().find(|j| j.id == id).unwrap().clone();
        assert!(
            job.task.contains("codeterm mem dream run"),
            "stale bare `mem dream` task must be healed to `mem dream run`, got: {:?}",
            job.task
        );
    }

    #[test]
    fn dream_cron_enabled_state_round_trips_through_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = CronStore::new_with_path(path);

        ensure_dreaming(&mut store).expect("create dream cron");
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();

        let enabled_now = |store: &CronStore, id: &str| -> bool {
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
}
