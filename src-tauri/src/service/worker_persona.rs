use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::multiplexer::types::AgentRole;
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

    fn reserved_titles(&self, scope: &ReservationScope) -> Vec<String> {
        self.entries
            .values()
            .filter(|entry| &entry.scope == scope)
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
        let mut used = self.used_worker_persona_names_locked(&scope);
        used.extend(reservations.reserved_titles(&scope));
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
                params.parent_pane_id.as_deref(),
                |pid| mux.find_workspace_for_pane(pid),
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

    fn used_worker_persona_names_locked(&self, scope: &ReservationScope) -> Vec<String> {
        let mut names = BTreeSet::new();
        {
            let mux = self.mux.read();
            for pane in mux
                .list_workspaces()
                .into_iter()
                .filter(|ws| ws.id == scope.workspace_id)
                .flat_map(|ws| ws.pane_ids)
                .filter_map(|pane_id| mux.get_pane(&pane_id))
            {
                if pane.role != Some(AgentRole::Worker) {
                    continue;
                }
                if let Some(target_session) = scope.session_id.as_deref() {
                    if pane.agent_session_id.as_deref() != Some(target_session) {
                        continue;
                    }
                }
                let title = mux.display_label_for_pane(&pane.id);
                if !title.trim().is_empty() {
                    names.insert(title);
                }
            }
        }
        names.extend(self.plan_store.recent_worker_attempt_titles(
            &scope.workspace_id,
            scope.session_id.as_deref(),
            RECENT_ATTEMPT_LIMIT,
        ));
        names.into_iter().collect()
    }
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
    params.parent_pane_id.hash(&mut hasher);
    params.spawned_by_pane_id.hash(&mut hasher);
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
