pub mod atomic_write;
pub mod attachments;
pub mod batch;
pub mod body_files;
pub mod canvas_elements;
pub mod cleanup_debt;
pub mod competition;
pub mod complete_wrapup;
pub mod convert_storage;
pub mod digest_rules;
pub mod element_queries;
pub mod external_refs;
pub mod gates;
pub mod handoff;
pub mod heartbeat;
pub mod hints;
pub mod ingest;
pub mod interruption;
pub mod lane_cleanup;
mod lane_metadata;
pub mod lifecycle;
pub mod locate;
pub mod loop_mutations;
pub mod migration_backup;
pub mod milestone_gates;
pub mod mutations;
pub mod next_actions;
pub mod paths;
pub mod peer_mandate;
pub mod plan_skills;
pub mod planner_requests;
pub mod ports;
pub mod prompts_plan;
pub mod qa;
mod readback;
mod reconcile_fixes;
pub mod reflection;
mod relink;
pub mod reminders;
pub mod resolve;
pub mod retrospective;
pub mod review_gate;
pub mod search_index;
pub mod search_related;
pub mod section_links;
pub mod section_queries;
pub mod skill_registry;
pub mod spec;
pub mod spec_render;
pub mod spec_store;
pub mod state;
mod store_db;
mod store_index;
mod store_persist;
mod store_queries;
pub mod store_v2;
mod store_watch;
pub mod stride_context;
pub mod stride_log;
pub mod tag_model;
pub mod task_attempts;
pub mod task_brief;
mod task_dispatch;
pub mod task_queries;
mod task_remove;
mod types;
pub mod usage_digest;
pub mod watcher;
pub mod worker_charter;
pub mod worker_identity;

#[cfg(test)]
mod attachment_tests;
#[cfg(test)]
mod body_file_tests;
#[cfg(test)]
mod canonical_path_tests;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod goal_tests;
#[cfg(test)]
mod heartbeat_tests;
#[cfg(test)]
mod readback_tests;
#[cfg(test)]
mod reminder_tests;
#[cfg(test)]
mod subtask_close_tests;
#[cfg(test)]
mod task_attempt_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timestamp_tests;
#[cfg(test)]
mod watch_reconcile_tests;

use crate::plan::{Plan, PlanMode, PlanStatus, Spec, TaskStatus};
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

pub use relink::relink_to_owner;
pub use state::{
    build_plan_state_snapshot, planning_approval_blockers, HeartbeatSnapshot, LiveAgent,
    LoopSnapshot, OpenLane, PlanStateSnapshot, PlanningReadiness, StrideSnapshot, TaskCounts,
    TaskPresentation,
};
pub use store_watch::WatchOutcome;
pub use types::{
    IndexEntry, NewTask, OpenSubtask, PlanError, ReconcileFixResult, SpecIndexEntry, TaskLaneUpdate,
};

pub struct PlanStore {
    plans: RwLock<HashMap<String, Arc<Mutex<Plan>>>>,
    specs: RwLock<HashMap<String, Arc<Mutex<Spec>>>>,
    index: RwLock<HashMap<String, IndexEntry>>,
    spec_index: RwLock<HashMap<String, SpecIndexEntry>>,
    last_writes: RwLock<HashMap<PathBuf, SystemTime>>,
    turn_counter: RwLock<Option<Arc<reminders::TurnCounter>>>,
    reseed_prepare_lock: Mutex<()>,
    mutation_writer: MutationWriter,
    database: Arc<store_db::PlanDatabase>,
    #[cfg(test)]
    persistence_hook: RwLock<Option<Arc<dyn Fn(&Plan) + Send + Sync>>>,
}

#[derive(Default)]
struct MutationWriter {
    state: Mutex<MutationWriterState>,
    ready: Condvar,
}

#[derive(Default)]
struct MutationWriterState {
    next_ticket: u64,
    serving_ticket: u64,
}

impl MutationWriter {
    fn enter(&self) -> MutationWriterTurn<'_> {
        let mut state = self.state.lock();
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        while state.serving_ticket != ticket {
            self.ready.wait(&mut state);
        }
        drop(state);
        MutationWriterTurn { writer: self }
    }
}

struct MutationWriterTurn<'a> {
    writer: &'a MutationWriter,
}

impl Drop for MutationWriterTurn<'_> {
    fn drop(&mut self) {
        let mut state = self.writer.state.lock();
        state.serving_ticket += 1;
        self.writer.ready.notify_all();
    }
}

impl PlanStore {
    pub fn new() -> Self {
        Self::open(paths::global_plan_database_path()).expect("open global PlanStore database")
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(root: &Path) -> Self {
        paths::mark_test_project_root(root);
        search_index::init_search_port(std::sync::Arc::new(test_support::TestSearchPort));
        Self::open(root.join(".codeterm").join("plans.db")).expect("open test PlanStore database")
    }

    pub fn open(database_path: PathBuf) -> Result<Self, PlanError> {
        let database = Arc::new(store_db::PlanDatabase::open(database_path)?);
        let (plans, specs) = database.load_snapshots()?;
        let store = Self {
            plans: RwLock::new(HashMap::new()),
            specs: RwLock::new(HashMap::new()),
            index: RwLock::new(HashMap::new()),
            spec_index: RwLock::new(HashMap::new()),
            last_writes: RwLock::new(HashMap::new()),
            turn_counter: RwLock::new(None),
            reseed_prepare_lock: Mutex::new(()),
            mutation_writer: MutationWriter::default(),
            database,
            #[cfg(test)]
            persistence_hook: RwLock::new(None),
        };
        for plan in plans {
            let id = plan.id.clone();
            let entry = Self::index_entry(&plan, store.database.path());
            store
                .plans
                .write()
                .insert(id.clone(), Arc::new(Mutex::new(plan)));
            store.index.write().insert(id, entry);
        }
        for spec in specs {
            let id = spec.id.clone();
            let entry = Self::spec_index_entry(&spec, store.database.path());
            store
                .specs
                .write()
                .insert(id.clone(), Arc::new(Mutex::new(spec)));
            store.spec_index.write().insert(id, entry);
        }
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn database_path_for_test(&self) -> PathBuf {
        self.database.path()
    }

    #[cfg(test)]
    pub(crate) fn operation_count_for_test(&self, entity_id: Option<&str>) -> usize {
        self.database.operation_count_for_test(entity_id)
    }
}
