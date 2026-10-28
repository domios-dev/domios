use crate::{
    curation::{
        calibration::stale::StaleParams,
        merge::{KindRestore, MergeError, MergeRequest},
    },
    store, MemManager,
};
impl MemManager {
    pub fn backend(&self) -> &dyn crate::backend::MemStore {
        &*self.store
    }
    pub fn merge_area(
        &self,
        from: &str,
        into: &str,
        dry_run: bool,
        dream_run: Option<&str>,
    ) -> Result<store::area_merge::AreaMergeOutcome, String> {
        self.store
            .merge_area(&Self::write_context(), from, into, dry_run, dream_run)
    }
    pub fn restore_area(&self, id: &str, version: Option<i64>) -> Result<Option<String>, String> {
        self.store.restore_area(&Self::write_context(), id, version)
    }
    pub fn merge_items(
        &self,
        req: &MergeRequest<'_>,
        actor: &str,
        cap: u32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, MergeError> {
        self.store
            .merge_items(&Self::write_context(), req, actor, cap, now)
    }
    pub fn restore_kind(
        &self,
        id: &str,
        restore: &KindRestore,
        body: Option<&str>,
        actor: &str,
    ) -> Result<serde_json::Value, String> {
        self.store
            .restore_kind(&Self::write_context(), id, restore, body, actor)
    }
    pub fn archive(
        &self,
        id: &str,
        reason: &str,
        actor: &str,
    ) -> Result<serde_json::Value, MergeError> {
        self.store
            .archive(&Self::write_context(), id, reason, actor)
    }
    pub fn is_pinned(&self, id: &str) -> Result<bool, String> {
        self.store.is_pinned(id)
    }
    pub fn calibration_sweep(&self, force: bool) -> Result<serde_json::Value, String> {
        self.store.calibration_sweep(&Self::write_context(), force)
    }
    pub fn calibration_log(
        &self,
        since: &str,
        limit: usize,
        count_only: bool,
    ) -> Result<serde_json::Value, String> {
        self.store.calibration_log(since, limit, count_only)
    }
    pub fn nightly_calibration(
        &self,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, String> {
        self.store
            .nightly_calibration(&Self::write_context(), hygiene, now)
    }
    pub fn stale(
        &self,
        p: &StaleParams,
        hygiene: &codeterm_types::config::DreamHygieneConfig,
    ) -> Result<serde_json::Value, String> {
        self.store.stale(&Self::write_context(), p, hygiene)
    }
    pub fn diary_page(&self, offset: usize, limit: usize) -> Result<serde_json::Value, String> {
        self.store.diary_page(offset, limit)
    }
    pub fn bump_on_get(&self, kind: &str, id: &str) {
        self.store.bump_on_get(&Self::write_context(), kind, id);
    }
    pub fn graph_type_counts(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>) {
        self.store.graph_type_counts()
    }
}

impl MemManager {
    pub fn restore_kind_as(
        &self,
        author: store::MemAuthor,
        id: &str,
        restore: &KindRestore,
        body: Option<&str>,
        actor: &str,
    ) -> Result<serde_json::Value, String> {
        self.store.restore_kind(
            &Self::write_context().with_author(author),
            id,
            restore,
            body,
            actor,
        )
    }
}

impl MemManager {
    pub fn sync_status(&self, timeout: std::time::Duration) -> Option<serde_json::Value> {
        self.store.sync_status(timeout)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn lock_storage(&self) -> Box<dyn crate::backend::StorageLock + '_> {
        self.store.lock_for_test()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_storage_lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.store.lock_for_test();
        f()
    }

    pub fn seed_graph(
        &self,
        entities: &[(crate::Entity, String)],
        relations: &[crate::Relation],
    ) -> Result<(), String> {
        self.store
            .seed_graph(&Self::write_context(), entities, relations)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl MemManager {
    pub fn fixture_set_kind(&self, id: &str, kind: &str) -> Result<(), String> {
        self.store
            .fixture_set_kind(&Self::write_context(), id, kind)
    }
}
