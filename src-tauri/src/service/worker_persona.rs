use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::service::SpawnIdentityProvenance;

use super::{spawn::resolve_spawn_workspace_id, ServiceContext, SpawnAgentParams};

const WORKER_PERSONA_POOL: &[&str] = &[
    "Ada", "Adele", "Alma", "Ansel", "Arlo", "Aster", "Bea", "Beno", "Bram", "Cora", "Dahl",
    "Darcy", "Edda", "Eli", "Elsa", "Enzo", "Esme", "Etta", "Faye", "Felix", "Finn", "Gael",
    "Hana", "Hugo", "Iris", "Jules", "Kai", "Kira", "Lena", "Levi", "Lina", "Luca", "Mara", "Milo",
    "Mira", "Nell", "Nico", "Nora", "Olin", "Oona", "Orin", "Otis", "Pia", "Remi", "Rhea", "Rina",
    "Sage", "Sana", "Tala", "Tess", "Theo", "Vera", "Vito", "Willa", "Yara", "Zane",
];
const DISALLOWED_AUTOMATIC_NAMES: &[&str] = &["Dijkstra"];
const FALLBACK_PREFIX: &str = "Worker";
const RECENT_ATTEMPT_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerPersonaAllocation {
    pub title: String,
    pub exhausted_pool: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ReservationScope {
    workspace_id: String,
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ReservationEntry {
    scope: ReservationScope,
    title: String,
}

#[derive(Debug, Default)]
pub(crate) struct WorkerPersonaReservations {
    next_id: u64,
    entries: BTreeMap<u64, ReservationEntry>,
}

pub(crate) struct WorkerPersonaReservationGuard {
    reservations: Arc<Mutex<WorkerPersonaReservations>>,
    reservation_id: Option<u64>,
}

impl Drop for WorkerPersonaReservationGuard {
    fn drop(&mut self) {
        if let Some(id) = self.reservation_id.take() {
            self.reservations.lock().release(id);
        }
    }
}

impl WorkerPersonaReservations {
    fn reserve(
        &mut self,
        scope: ReservationScope,
        title: String,
    ) -> WorkerPersonaReservationGuardToken {
        self.next_id += 1;
        let id = self.next_id;
        self.entries.insert(id, ReservationEntry { scope, title });
        WorkerPersonaReservationGuardToken(id)
    }

    fn release(&mut self, reservation_id: u64) {
        self.entries.remove(&reservation_id);
    }

    #[cfg(test)]
    fn reserved_titles(&self, scope: &ReservationScope) -> Vec<String> {
        self.entries
            .values()
            .filter(|entry| &entry.scope == scope)
            .map(|entry| entry.title.clone())
            .collect()
    }

    /// Reservations across the whole workspace, regardless of which session
    /// made them — closes the cross-session concurrent-claim race.
    fn reserved_titles_in_workspace(&self, workspace_id: &str) -> Vec<String> {
        self.entries
            .values()
            .filter(|entry| entry.scope.workspace_id == workspace_id)
            .map(|entry| entry.title.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct WorkerPersonaReservationGuardToken(u64);

impl ServiceContext {
    pub(crate) fn reserve_worker_persona(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<Option<WorkerPersonaReservationGuard>, String> {
        let intent = self.worker_persona_intent(params)?;
        let Some(intent) = intent else {
            return Ok(None);
        };
        let scope = self.worker_persona_scope(params)?;
        let seed = seed_for_worker_persona(params, &scope, intent.suggested_title.as_deref());
        let mut reservations = self.worker_identity_allocator.lock();
        let mut used = self.used_worker_persona_names_locked(params, &scope);
        // Workspace-wide, not session-scoped: two sessions racing in one
        // workspace must never claim the same in-flight persona.
        used.extend(reservations.reserved_titles_in_workspace(&scope.workspace_id));
        let allocation = allocate_worker_persona_name(
            &used,
            intent.suggested_title.as_deref(),
            Some(FALLBACK_PREFIX),
            seed,
        );
        let token = reservations.reserve(scope, allocation.title.clone());
        drop(reservations);
        params.title = Some(allocation.title);
        params.title_provenance = Some(SpawnIdentityProvenance::Suggested);
        Ok(Some(WorkerPersonaReservationGuard {
            reservations: self.worker_identity_allocator.clone(),
            reservation_id: Some(token.0),
        }))
    }

    fn worker_persona_intent(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<Option<WorkerPersonaIntent>, String> {
        if params.role.as_deref() != Some("Worker") {
            return Ok(None);
        }
        if has_explicit_identity(params.agent_name.as_deref()) {
            return Ok(None);
        }
        match params
            .title_provenance
            .unwrap_or(SpawnIdentityProvenance::Suggested)
        {
            SpawnIdentityProvenance::UserSupplied => Ok(None),
            SpawnIdentityProvenance::Suggested => Ok(Some(WorkerPersonaIntent {
                suggested_title: params.title.take(),
            })),
        }
    }

    fn worker_persona_scope(&self, params: &SpawnAgentParams) -> Result<ReservationScope, String> {
        let workspace_id = {
            let mux = self.mux.read();
            resolve_spawn_workspace_id(
                params.workspace_id.clone(),
                params.parent_tab_id.as_deref(),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?
        };
        Ok(ReservationScope {
            workspace_id,
            session_id: params
                .resume_session_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        })
    }

    /// Titles a new worker persona must never collide with: every live tab in
    /// the workspace (any role, any session), the spawning parent, and recent attempts.
    fn used_worker_persona_names_locked(
        &self,
        params: &SpawnAgentParams,
        scope: &ReservationScope,
    ) -> Vec<String> {
        let mut names = {
            let mux = self.mux.read();
            let mut names = workspace_live_titles(&mux, &scope.workspace_id);
            if let Some(parent_id) = params.parent_tab_id.as_deref() {
                let title = mux.display_label_for_tab(parent_id);
                if !title.trim().is_empty() {
                    names.insert(title);
                }
            }
            names
        };
        names.extend(self.plan_store.recent_worker_attempt_titles(
            &scope.workspace_id,
            scope.session_id.as_deref(),
            RECENT_ATTEMPT_LIMIT,
        ));
        names.into_iter().collect()
    }
}

/// Every non-empty display title of a live tab in `workspace_id`, regardless
/// of role or owning session.
fn workspace_live_titles(
    mux: &crate::multiplexer::state::MultiplexerState,
    workspace_id: &str,
) -> BTreeSet<String> {
    mux.list_workspaces()
        .into_iter()
        .filter(|ws| ws.id == workspace_id)
        .flat_map(|ws| ws.tab_ids)
        .filter(|tab_id| mux.get_tab(tab_id).is_some())
        .map(|tab_id| mux.display_label_for_tab(&tab_id))
        .filter(|title| !title.trim().is_empty())
        .collect()
}

struct WorkerPersonaIntent {
    suggested_title: Option<String>,
}

pub(crate) fn allocate_worker_persona_name<I, S>(
    used_names: I,
    preferred_title: Option<&str>,
    preferred_base: Option<&str>,
    seed: u64,
) -> WorkerPersonaAllocation
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let used = normalize_used_names(used_names);
    if let Some(preferred) = preferred_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !is_disallowed_automatic_name(value))
    {
        if WORKER_PERSONA_POOL
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(preferred))
            && !used.contains(&canonical_name(preferred))
        {
            return WorkerPersonaAllocation {
                title: preferred.to_string(),
                exhausted_pool: false,
            };
        }
    }

    if let Some(title) = select_pool_name(&used, seed) {
        return WorkerPersonaAllocation {
            title,
            exhausted_pool: false,
        };
    }

    let base = preferred_base
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(FALLBACK_PREFIX);
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{base} {suffix}");
        if !used.contains(&canonical_name(&candidate)) {
            return WorkerPersonaAllocation {
                title: candidate,
                exhausted_pool: true,
            };
        }
        suffix += 1;
    }
}

fn select_pool_name(used: &HashSet<String>, seed: u64) -> Option<String> {
    if WORKER_PERSONA_POOL.is_empty() {
        return None;
    }
    let start = (seed as usize) % WORKER_PERSONA_POOL.len();
    for offset in 0..WORKER_PERSONA_POOL.len() {
        let candidate = WORKER_PERSONA_POOL[(start + offset) % WORKER_PERSONA_POOL.len()];
        if is_disallowed_automatic_name(candidate) {
            continue;
        }
        if !used.contains(&canonical_name(candidate)) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn is_disallowed_automatic_name(value: &str) -> bool {
    DISALLOWED_AUTOMATIC_NAMES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(value.trim()))
}

fn has_explicit_identity(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
}

fn normalize_used_names<I, S>(used_names: I) -> HashSet<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    used_names
        .into_iter()
        .map(|name| canonical_name(name.as_ref()))
        .filter(|name| !name.is_empty())
        .collect()
}

fn canonical_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn seed_for_worker_persona(
    params: &SpawnAgentParams,
    scope: &ReservationScope,
    suggested_title: Option<&str>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    scope.hash(&mut hasher);
    params.plan_id.hash(&mut hasher);
    params.task_id.hash(&mut hasher);
    params.plan_task.hash(&mut hasher);
    params.parent_tab_id.hash(&mut hasher);
    params.spawned_by_tab_id.hash(&mut hasher);
    params.cwd.hash(&mut hasher);
    suggested_title.map(str::trim).hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::SpawnIdentityProvenance;
    use std::sync::{Arc, Barrier};

    fn scope() -> ReservationScope {
        ReservationScope {
            workspace_id: "ws".into(),
            session_id: Some("sess".into()),
        }
    }

    #[test]
    fn historical_dijkstra_is_never_auto_assigned() {
        let allocation = allocate_worker_persona_name(["Ada"], Some("Dijkstra"), None, 0);
        assert_ne!(allocation.title, "Dijkstra");
    }

    #[test]
    fn provisional_suggestion_collision_rerolls() {
        let allocation = allocate_worker_persona_name(["Ada"], Some("Ada"), None, 0);
        assert_ne!(allocation.title, "Ada");
        assert!(!allocation.exhausted_pool);
    }

    #[test]
    fn explicit_user_collision_is_preserved() {
        let params = SpawnAgentParams {
            role: Some("Worker".into()),
            title: Some("Ada".into()),
            title_provenance: Some(SpawnIdentityProvenance::UserSupplied),
            ..Default::default()
        };
        assert_eq!(params.title.as_deref(), Some("Ada"));
        assert_eq!(
            params.title_provenance,
            Some(SpawnIdentityProvenance::UserSupplied)
        );
    }

    #[test]
    fn deterministic_variation_changes_starting_pick() {
        let first = allocate_worker_persona_name(std::iter::empty::<&str>(), None, None, 0);
        let second = allocate_worker_persona_name(std::iter::empty::<&str>(), None, None, 7);
        assert_ne!(first.title, second.title);
    }

    #[test]
    fn suffix_fallback_only_after_pool_exhaustion() {
        let allocation = allocate_worker_persona_name(WORKER_PERSONA_POOL, None, None, 0);
        assert_eq!(allocation.title, "Worker 2");
        assert!(allocation.exhausted_pool);
    }

    #[test]
    fn concurrent_claims_produce_twenty_unique_names_without_suffix_or_dijkstra() {
        let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
        let barrier = Arc::new(Barrier::new(20));
        let mut threads = Vec::new();
        for idx in 0..20u64 {
            let reservations = reservations.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                let mut lock = reservations.lock();
                let mut used = lock.reserved_titles(&scope());
                let allocation = allocate_worker_persona_name(&used, None, None, idx);
                let token = lock.reserve(scope(), allocation.title.clone());
                used.push(allocation.title.clone());
                (allocation, token.0)
            }));
        }
        let mut seen = HashSet::new();
        let mut reservation_ids = Vec::new();
        for join in threads {
            let (allocation, reservation_id) = join.join().expect("thread join");
            assert!(seen.insert(allocation.title.clone()));
            assert_ne!(allocation.title, "Dijkstra");
            assert!(!allocation.title.starts_with("Worker "));
            reservation_ids.push(reservation_id);
        }
        assert_eq!(seen.len(), 20);
        let mut lock = reservations.lock();
        for reservation_id in reservation_ids {
            lock.release(reservation_id);
        }
        assert!(lock.entries.is_empty());
    }

    #[test]
    fn workspace_live_titles_excludes_non_worker_roles() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::AgentRole;

        let (mut mux, orch_tab) = MultiplexerState::new();
        let ws_id = mux
            .find_workspace_for_tab(&orch_tab)
            .expect("initial Tab has a workspace");
        mux.rename_tab(&orch_tab, "Orchestrator-Prime")
            .expect("rename orchestrator Tab");

        let (_tab_id, plain_tab) = mux.create_tab(&ws_id).expect("create plain tab");
        mux.rename_tab(&plain_tab, "Kai").expect("rename Tab");

        let (_tab_id, worker_tab) = mux.create_tab(&ws_id).expect("create worker tab");
        mux.set_tab_role(&worker_tab, AgentRole::Worker, Some(&orch_tab))
            .expect("attach worker");
        mux.rename_tab(&worker_tab, "Esme")
            .expect("rename worker");

        let titles = workspace_live_titles(&mux, &ws_id);
        assert!(
            titles.contains("Orchestrator-Prime"),
            "the parent Orchestrator's own tab title must count as used, not just Worker Tabs: {titles:?}"
        );
        assert!(
            titles.contains("Kai"),
            "a plain (roleless) tab's title must count as used: {titles:?}"
        );
        assert!(titles.contains("Esme"));
    }

    #[test]
    fn workspace_live_titles_ignores_session_scoping() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::AgentRole;

        let (mut mux, orch_tab) = MultiplexerState::new();
        let ws_id = mux.find_workspace_for_tab(&orch_tab).unwrap();

        let (_tab_id, other_session_worker) = mux.create_tab(&ws_id).unwrap();
        mux.set_tab_role(&other_session_worker, AgentRole::Worker, Some(&orch_tab))
            .unwrap();
        mux.rename_tab(&other_session_worker, "Nora").unwrap();
        if let Some(tab) = mux.get_tab_mut(&other_session_worker) {
            tab.agent_session_id = Some("some-other-session".into());
        }

        // The old scan filtered live tabs to the caller's own session_id,
        // so a Worker title claimed under a different session was invisible.
        let titles = workspace_live_titles(&mux, &ws_id);
        assert!(
            titles.contains("Nora"),
            "a title held by a Tab from a different session must still be excluded: {titles:?}"
        );
    }

    #[test]
    fn allocation_rerolls_away_from_parent_and_live_titles() {
        // Once the exclusion set includes the parent + live tabs, a suggested
        // pick must reroll exactly like an in-pool collision.
        let used = ["Orchestrator-Prime", "Kai"];
        let allocation = allocate_worker_persona_name(used, Some("Kai"), None, 0);
        assert_ne!(allocation.title, "Kai");
        assert!(!allocation.exhausted_pool);
    }

    #[test]
    fn reservations_collide_across_sessions_in_the_same_workspace() {
        let mut reservations = WorkerPersonaReservations::default();
        let scope_a = ReservationScope {
            workspace_id: "ws".into(),
            session_id: Some("session-a".into()),
        };
        let scope_b = ReservationScope {
            workspace_id: "ws".into(),
            session_id: Some("session-b".into()),
        };
        reservations.reserve(scope_a, "Ada".into());

        // Session-scoped lookup (the old collision-check) can't see session-a's
        // in-flight claim from session-b's scope.
        assert!(reservations.reserved_titles(&scope_b).is_empty());
        // Workspace-scoped lookup sees it regardless of which session reserved it.
        assert_eq!(
            reservations.reserved_titles_in_workspace("ws"),
            vec!["Ada".to_string()]
        );
    }

    #[test]
    fn failed_spawn_rollback_releases_reservation() {
        let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
        {
            let mut lock = reservations.lock();
            let allocation =
                allocate_worker_persona_name(std::iter::empty::<&str>(), None, None, 0);
            let token = lock.reserve(scope(), allocation.title.clone());
            drop(lock);
            let guard = WorkerPersonaReservationGuard {
                reservations: reservations.clone(),
                reservation_id: Some(token.0),
            };
            assert_eq!(reservations.lock().entries.len(), 1);
            drop(guard);
        }
        let lock = reservations.lock();
        let allocation =
            allocate_worker_persona_name(lock.reserved_titles(&scope()), None, None, 0);
        assert_eq!(allocation.title, WORKER_PERSONA_POOL[0]);
        assert!(lock.entries.is_empty());
    }
}
