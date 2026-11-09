use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::service::SpawnIdentityProvenance;

use super::{spawn::resolve_spawn_workspace_id, ServiceContext, SpawnAgentParams};

const WORKER_PERSONA_POOL: &[&str] = &[
    "Aaron", "Abby", "Adam", "Alan", "Albert", "Alex", "Alice", "Amy", "Andy", "Anita", "Anna",
    "April", "Arthur", "Ava", "Bella", "Ben", "Betty", "Blake", "Bob", "Brian", "Bruce", "Carl",
    "Carol", "Chloe", "Chris", "Clara", "Colin", "Daisy", "Daniel", "David", "Dean", "Derek",
    "Diana", "Dylan", "Edward", "Elena", "Elise", "Ella", "Emma", "Emily", "Eric", "Ethan", "Eva",
    "Felix", "Fiona", "Frank", "Fred", "Gary", "Gavin", "George", "Gina", "Grace", "Greg",
    "Hannah", "Harry", "Hazel", "Helen", "Henry", "Holly", "Ian", "Irene", "Isaac", "Jack",
    "James", "Jane", "Jason", "Jean", "Jenny", "Jesse", "Joel", "John", "Jordan", "Joy", "Julia",
    "Karen", "Kate", "Kevin", "Lana", "Lara", "Laura", "Leah", "Leo", "Liam", "Lily", "Linda",
    "Lisa", "Logan", "Louis", "Lucas", "Lucy", "Luke", "Mabel", "Maria", "Mark", "Martin", "Mary",
    "Max", "Megan", "Mia", "Mike", "Molly", "Nancy", "Neil", "Nina", "Noah", "Nora", "Oliver",
    "Oscar", "Owen", "Pam", "Patrick", "Paul", "Peter", "Rachel", "Rita", "Robert", "Robin",
    "Rose", "Ruby", "Ryan", "Sam", "Sara", "Scott", "Simon", "Stella", "Steve", "Susan", "Ted",
    "Terry", "Tina", "Toby", "Tom", "Tony", "Tyler", "Vera", "Victor", "Wendy", "Zoe",
];
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
    #[allow(dead_code)]
    scope: ReservationScope,
    title: String,
    /// Set once the persona is handed to a tab: the claim is then keyed on the
    /// tab id and survives any later rename of that tab's display title.
    tab_id: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct WorkerPersonaReservations {
    next_id: u64,
    entries: BTreeMap<u64, ReservationEntry>,
}

pub(crate) struct WorkerIdentityAllocator(
    pub(crate) Arc<parking_lot::Mutex<WorkerPersonaReservations>>,
);

pub(crate) struct WorkerPersonaReservationGuard {
    reservations: Arc<Mutex<WorkerPersonaReservations>>,
    reservation_id: Option<u64>,
}

impl WorkerPersonaReservationGuard {
    /// Hand the persona to `tab_id` for that tab's lifetime, disarming the
    /// spawn-window release. Identity is keyed on the tab id from here on, so a
    /// self-rename relabels the tab without releasing its persona.
    pub(crate) fn commit_to_tab(mut self, tab_id: &str) {
        if let Some(id) = self.reservation_id.take() {
            self.reservations.lock().commit(id, tab_id);
        }
    }
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
        self.entries.insert(
            id,
            ReservationEntry {
                scope,
                title,
                tab_id: None,
            },
        );
        WorkerPersonaReservationGuardToken(id)
    }

    fn release(&mut self, reservation_id: u64) {
        self.entries.remove(&reservation_id);
    }

    /// Bind a reservation to the tab now carrying the persona, so the claim
    /// outlives the spawn window.
    fn commit(&mut self, reservation_id: u64, tab_id: &str) {
        if let Some(entry) = self.entries.get_mut(&reservation_id) {
            entry.tab_id = Some(tab_id.to_string());
        }
    }

    fn release_for_tab(&mut self, tab_id: &str) {
        self.entries
            .retain(|_, entry| entry.tab_id.as_deref() != Some(tab_id));
    }

    #[cfg(test)]
    fn reserved_titles(&self, scope: &ReservationScope) -> Vec<String> {
        self.entries
            .values()
            .filter(|entry| &entry.scope == scope)
            .map(|entry| entry.title.clone())
            .collect()
    }

    /// Reservations across every workspace and session: an in-flight claim is
    /// global, so no second Worker anywhere may take the same name.
    fn all_reserved_titles(&self) -> Vec<String> {
        self.entries
            .values()
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
        let mut reservations = self
            .ext::<crate::service::WorkerIdentityAllocator>()
            .0
            .lock();
        let mut used = self.used_worker_persona_names_locked(params, &scope)?;
        // Global, not workspace-scoped: a Worker spawning in any workspace must
        // never claim a persona another in-flight Worker already holds.
        used.extend(reservations.all_reserved_titles());
        let allocation = if intent.preserve_title {
            allocate_task_bound_persona_name(intent.suggested_title.as_deref(), &used, seed)
        } else {
            allocate_worker_persona_name(
                &used,
                intent.suggested_title.as_deref(),
                Some(FALLBACK_PREFIX),
                seed,
            )
        };
        let token = reservations.reserve(scope, allocation.title.clone());
        drop(reservations);
        params.title = Some(allocation.title);
        params.title_provenance = Some(SpawnIdentityProvenance::Suggested);
        Ok(Some(WorkerPersonaReservationGuard {
            reservations: self
                .ext::<crate::service::WorkerIdentityAllocator>()
                .0
                .clone(),
            reservation_id: Some(token.0),
        }))
    }

    /// Release the persona held by a closing tab, returning it to the pool.
    pub(crate) fn release_worker_persona_for_tab(&self, tab_id: &str) {
        self.ext::<crate::service::WorkerIdentityAllocator>()
            .0
            .lock()
            .release_for_tab(tab_id);
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
        let preserve_title = params.plan_task.is_some() || params.task_launch_authority.is_some();
        match params
            .title_provenance
            .unwrap_or(SpawnIdentityProvenance::Suggested)
        {
            SpawnIdentityProvenance::UserSupplied if !preserve_title => Ok(None),
            SpawnIdentityProvenance::Suggested => Ok(Some(WorkerPersonaIntent {
                suggested_title: params.title.take(),
                preserve_title,
            })),
            SpawnIdentityProvenance::UserSupplied => Ok(Some(WorkerPersonaIntent {
                suggested_title: params.title.take(),
                preserve_title: true,
            })),
        }
    }

    fn worker_persona_scope(&self, params: &SpawnAgentParams) -> Result<ReservationScope, String> {
        let workspace_id = {
            let mux = self.mux.read();
            resolve_spawn_workspace_id(
                params.workspace_id.clone(),
                params.parent_tab_id.as_deref(),
                crate::service::spawn::params_caller_tab_id(params),
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
    /// every workspace (any role, any session), the spawning parent, and recent attempts.
    fn used_worker_persona_names_locked(
        &self,
        params: &SpawnAgentParams,
        scope: &ReservationScope,
    ) -> Result<Vec<String>, String> {
        let mut names = {
            let mux = self.mux.read();
            let mut names = all_live_titles(&mux);
            if let Some(parent_id) = params.parent_tab_id.as_deref() {
                let title = mux.display_label_for_tab(parent_id);
                if !title.trim().is_empty() {
                    names.insert(title);
                }
            }
            names
        };
        names.extend(
            self.plan_store
                .recent_worker_attempt_titles(
                    &scope.workspace_id,
                    scope.session_id.as_deref(),
                    RECENT_ATTEMPT_LIMIT,
                )
                .map_err(|error| error.to_string())?,
        );
        Ok(names.into_iter().collect())
    }
}

/// Every non-empty display title of a live tab across every workspace,
/// regardless of role or owning session. Persona uniqueness is global: the same
/// one-word name must never label two live tabs, even in different workspaces.
fn all_live_titles(mux: &crate::multiplexer::state::MultiplexerState) -> BTreeSet<String> {
    mux.list_workspaces()
        .into_iter()
        .flat_map(|ws| ws.tab_ids)
        .filter(|tab_id| mux.get_tab(tab_id).is_some())
        .map(|tab_id| mux.display_label_for_tab(&tab_id))
        .filter(|title| !title.trim().is_empty())
        .collect()
}

struct WorkerPersonaIntent {
    suggested_title: Option<String>,
    preserve_title: bool,
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
        let candidate = format!("{base}{suffix}");
        if !used.contains(&canonical_name(&candidate)) {
            return WorkerPersonaAllocation {
                title: candidate,
                exhausted_pool: true,
            };
        }
        suffix += 1;
    }
}

/// A task-bound Worker keeps its explicitly planned name verbatim; with no
/// planned name it draws a unique one from the pool rather than settling on the
/// bare role label. The in-flight claim is reserved by the caller either way.
fn allocate_task_bound_persona_name<I, S>(
    preferred_title: Option<&str>,
    used_names: I,
    seed: u64,
) -> WorkerPersonaAllocation
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if let Some(title) = preferred_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return WorkerPersonaAllocation {
            title: title.to_string(),
            exhausted_pool: false,
        };
    }
    allocate_worker_persona_name(used_names, None, Some(FALLBACK_PREFIX), seed)
}

fn select_pool_name(used: &HashSet<String>, seed: u64) -> Option<String> {
    if WORKER_PERSONA_POOL.is_empty() {
        return None;
    }
    let start = (seed as usize) % WORKER_PERSONA_POOL.len();
    for offset in 0..WORKER_PERSONA_POOL.len() {
        let candidate = WORKER_PERSONA_POOL[(start + offset) % WORKER_PERSONA_POOL.len()];
        if !used.contains(&canonical_name(candidate)) {
            return Some(candidate.to_string());
        }
    }
    None
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
mod tests;
