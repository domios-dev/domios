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
        let mut reservations = self.worker_identity_allocator.lock();
        let mut used = self.used_worker_persona_names_locked(params, &scope);
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
            reservations: self.worker_identity_allocator.clone(),
            reservation_id: Some(token.0),
        }))
    }

    /// Release the persona held by a closing tab, returning it to the pool.
    pub(crate) fn release_worker_persona_for_tab(&self, tab_id: &str) {
        self.worker_identity_allocator
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
    ) -> Vec<String> {
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
        names.extend(self.plan_store.recent_worker_attempt_titles(
            &scope.workspace_id,
            scope.session_id.as_deref(),
            RECENT_ATTEMPT_LIMIT,
        ));
        names.into_iter().collect()
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
    fn automatic_name_is_selected_from_the_pool() {
        let allocation = allocate_worker_persona_name(["Alex"], None, None, 0);
        assert!(WORKER_PERSONA_POOL.contains(&allocation.title.as_str()));
    }

    #[test]
    fn provisional_suggestion_collision_rerolls() {
        let allocation = allocate_worker_persona_name(["Ada"], Some("Ada"), None, 0);
        assert_ne!(allocation.title, "Ada");
        assert!(!allocation.exhausted_pool);
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn task_bound_planned_name_survives_persona_intent_and_allocation() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            plan_task: Some("task-bound-name".into()),
            title: Some("Eli".into()),
            title_provenance: Some(SpawnIdentityProvenance::Suggested),
            ..Default::default()
        };

        let reservation = fixture
            .service
            .reserve_worker_persona(&mut params)
            .expect("persona reservation succeeds");

        assert!(reservation.is_some());
        assert_eq!(params.title.as_deref(), Some("Eli"));
    }

    #[test]
    fn task_bound_without_planned_name_draws_from_the_pool() {
        let used = ["Orchestrator-Prime", "Kai"];
        let allocation = allocate_task_bound_persona_name(None, used, 0);
        assert!(
            WORKER_PERSONA_POOL.contains(&allocation.title.as_str()),
            "a task-bound Worker with no planned name must draw from the pool, got {}",
            allocation.title
        );
        assert_ne!(allocation.title, "Worker");
    }

    #[test]
    fn task_bound_planned_name_is_preserved_verbatim() {
        let allocation =
            allocate_task_bound_persona_name(Some("Ada"), std::iter::empty::<&str>(), 0);
        assert_eq!(allocation.title, "Ada");
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn task_bound_dispatch_without_persona_reserves_a_unique_pool_name() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            task_id: Some("task_bound".into()),
            task_launch_authority: Some(crate::service::TaskLaunchAuthority {
                plan_id: "plan_1".into(),
                task_id: "task_bound".into(),
                attempt_id: "attempt_1".into(),
                lane_base: None,
            }),
            ..Default::default()
        };

        let reservation = fixture
            .service
            .reserve_worker_persona(&mut params)
            .expect("persona reservation succeeds");

        assert!(reservation.is_some());
        let title = params.title.as_deref().expect("task-bound Worker is named");
        assert!(
            WORKER_PERSONA_POOL.contains(&title),
            "dispatch with no persona must name the Worker from the pool, got {title}"
        );
        assert_eq!(
            params.title_provenance,
            Some(SpawnIdentityProvenance::Suggested)
        );
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn task_bound_explicit_personality_wins_over_pool() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let mut params = SpawnAgentParams {
            role: Some("Worker".into()),
            task_id: Some("task_bound".into()),
            title: Some("Dumas".into()),
            title_provenance: Some(SpawnIdentityProvenance::UserSupplied),
            task_launch_authority: Some(crate::service::TaskLaunchAuthority {
                plan_id: "plan_1".into(),
                task_id: "task_bound".into(),
                attempt_id: "attempt_1".into(),
                lane_base: None,
            }),
            ..Default::default()
        };

        let reservation = fixture
            .service
            .reserve_worker_persona(&mut params)
            .expect("persona reservation succeeds");

        assert!(reservation.is_some());
        assert_eq!(params.title.as_deref(), Some("Dumas"));
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn in_flight_persona_is_not_reissued_in_another_workspace() {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let ws_a = mux.list_workspaces()[0].id.clone();
        let ws_b = mux.create_workspace("Other".into(), None).id.clone();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

        let mut first = SpawnAgentParams {
            role: Some("Worker".into()),
            workspace_id: Some(ws_a),
            ..Default::default()
        };
        let _first_guard = fixture
            .service
            .reserve_worker_persona(&mut first)
            .expect("persona reservation succeeds")
            .expect("a plain Worker is named");
        let first_name = first.title.clone().expect("first Worker is named");

        let mut second = SpawnAgentParams {
            role: Some("Worker".into()),
            workspace_id: Some(ws_b),
            title: Some(first_name.clone()),
            ..Default::default()
        };
        let _second_guard = fixture
            .service
            .reserve_worker_persona(&mut second)
            .expect("persona reservation succeeds")
            .expect("a plain Worker is named");

        assert_ne!(
            second.title.as_deref(),
            Some(first_name.as_str()),
            "an in-flight persona must not be reissued in another workspace"
        );
    }

    #[test]
    fn allocation_avoids_titles_held_in_another_workspace() {
        use crate::multiplexer::state::MultiplexerState;

        let (mut mux, _) = MultiplexerState::new();
        let ws_b = mux.create_workspace("Other".into(), None).id.clone();
        let tab_b = mux
            .create_tab(&ws_b)
            .expect("create tab in other workspace");
        mux.rename_tab(&tab_b, "Ada").expect("rename tab");

        let used = all_live_titles(&mux);
        let allocation = allocate_worker_persona_name(&used, Some("Ada"), None, 0);
        assert_ne!(
            allocation.title, "Ada",
            "a persona held in another workspace must not be reissued"
        );
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
    fn tab_name_contract_suffix_fallback_only_after_pool_exhaustion() {
        let allocation = allocate_worker_persona_name(WORKER_PERSONA_POOL, None, None, 0);
        assert_eq!(allocation.title, "Worker2");
        assert!(allocation.exhausted_pool);
    }

    #[test]
    fn tab_name_contract_spawn_name_paths_keep_personality_and_role_separate() {
        let task: crate::plan::Task = serde_json::from_value(serde_json::json!({
            "id": "task_name_contract",
            "name": "Stable identity",
            "type": "code",
            "status": "pending",
            "personality": "Ada",
            "executor_role": "rust-backend-dev"
        }))
        .expect("task fixture deserializes");
        assert_eq!(
            crate::service::planned_task_title(&task).as_deref(),
            Some("Ada")
        );

        let role_only: crate::plan::Task = serde_json::from_value(serde_json::json!({
            "id": "task_role_only",
            "name": "Role stays descriptive",
            "type": "code",
            "status": "pending",
            "executor_role": "rust-backend-dev"
        }))
        .expect("task fixture deserializes");
        assert_eq!(crate::service::planned_task_title(&role_only), None);
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
            assert!(WORKER_PERSONA_POOL.contains(&allocation.title.as_str()));
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
    fn pane_self_rename_does_not_release_a_live_worker_persona() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::AgentRole;

        let (mut mux, orch_tab) = MultiplexerState::new();
        let ws_id = mux.find_workspace_for_tab(&orch_tab).unwrap();
        mux.rename_tab(&orch_tab, "Holst").unwrap();

        let worker = mux.create_tab(&ws_id).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch_tab))
            .unwrap();

        // Spawn allocates the persona, applies it as the creation title, and
        // commits the claim to the tab id.
        let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
        let token = reservations.lock().reserve(
            ReservationScope {
                workspace_id: ws_id.clone(),
                session_id: None,
            },
            "Vera".into(),
        );
        WorkerPersonaReservationGuard {
            reservations: reservations.clone(),
            reservation_id: Some(token.0),
        }
        .commit_to_tab(&worker);
        mux.rename_tab(&worker, "Vera").unwrap();

        // The pane renames its own tab mid-task — the documented session-start rule.
        mux.rename_tab(&worker, "pane rename identity").unwrap();

        // Exactly what used_worker_persona_names_locked composes.
        let mut reserved = all_live_titles(&mux);
        reserved.extend(reservations.lock().all_reserved_titles());
        assert!(
            reserved.contains("Vera"),
            "a live Worker's assigned persona must stay reserved across its own rename; reserved={reserved:?}"
        );
        assert_ne!(
            allocate_worker_persona_name(&reserved, Some("Vera"), None, 0).title,
            "Vera",
            "persona reissued to a second Worker while the first is still live"
        );
    }

    #[test]
    fn closing_a_renamed_worker_returns_its_persona_to_the_pool() {
        let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
        let token = reservations.lock().reserve(scope(), "Vera".into());
        WorkerPersonaReservationGuard {
            reservations: reservations.clone(),
            reservation_id: Some(token.0),
        }
        .commit_to_tab("tab-1");
        assert_eq!(
            reservations.lock().all_reserved_titles(),
            vec!["Vera".to_string()]
        );

        reservations.lock().release_for_tab("tab-1");
        assert!(
            reservations.lock().all_reserved_titles().is_empty(),
            "a closed tab must not keep holding its persona"
        );
    }

    #[test]
    fn live_titles_include_all_roles_and_workspaces() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::AgentRole;

        let (mut mux, orch_tab) = MultiplexerState::new();
        let ws_id = mux
            .find_workspace_for_tab(&orch_tab)
            .expect("initial Tab has a workspace");
        mux.rename_tab(&orch_tab, "Orchestrator-Prime")
            .expect("rename orchestrator Tab");

        let plain_tab = mux.create_tab(&ws_id).expect("create plain tab");
        mux.rename_tab(&plain_tab, "Kai").expect("rename Tab");

        let worker_tab = mux.create_tab(&ws_id).expect("create worker tab");
        mux.set_tab_role(&worker_tab, AgentRole::Worker, Some(&orch_tab))
            .expect("attach worker");
        mux.rename_tab(&worker_tab, "Esme").expect("rename worker");

        let other_ws = mux.create_workspace("Other".into(), None).id.clone();
        let other_tab = mux.create_tab(&other_ws).expect("create tab in other ws");
        mux.rename_tab(&other_tab, "Nora").expect("rename tab");

        let titles = all_live_titles(&mux);
        assert!(
            titles.contains("Orchestrator-Prime"),
            "the parent Orchestrator's own tab title must count as used, not just Worker Tabs: {titles:?}"
        );
        assert!(
            titles.contains("Kai"),
            "a plain (roleless) tab's title must count as used: {titles:?}"
        );
        assert!(titles.contains("Esme"));
        assert!(
            titles.contains("Nora"),
            "a title held in a different workspace must count as used: {titles:?}"
        );
    }

    #[test]
    fn live_titles_ignore_session_scoping() {
        use crate::multiplexer::state::MultiplexerState;
        use crate::multiplexer::types::AgentRole;

        let (mut mux, orch_tab) = MultiplexerState::new();
        let ws_id = mux.find_workspace_for_tab(&orch_tab).unwrap();

        let other_session_worker = mux.create_tab(&ws_id).unwrap();
        mux.set_tab_role(&other_session_worker, AgentRole::Worker, Some(&orch_tab))
            .unwrap();
        mux.rename_tab(&other_session_worker, "Nora").unwrap();
        if let Some(tab) = mux.get_tab_mut(&other_session_worker) {
            tab.agent_session_id = Some("some-other-session".into());
        }

        // The old scan filtered live tabs to the caller's own session_id,
        // so a Worker title claimed under a different session was invisible.
        let titles = all_live_titles(&mux);
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
    fn reservations_are_global_across_sessions_and_workspaces() {
        let mut reservations = WorkerPersonaReservations::default();
        reservations.reserve(
            ReservationScope {
                workspace_id: "ws-a".into(),
                session_id: Some("session-a".into()),
            },
            "Ada".into(),
        );

        // A session-scoped lookup cannot see another session's claim...
        assert!(reservations
            .reserved_titles(&ReservationScope {
                workspace_id: "ws-a".into(),
                session_id: Some("session-b".into()),
            })
            .is_empty());
        // ...but the global claim scan must, regardless of session or workspace.
        assert_eq!(reservations.all_reserved_titles(), vec!["Ada".to_string()]);
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
