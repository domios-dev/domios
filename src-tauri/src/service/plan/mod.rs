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
pub mod gates;
pub mod handoff;
pub mod heartbeat;
pub mod hints;
pub mod ingest;
pub mod lane_cleanup;
mod lane_metadata;
pub mod lifecycle;
pub mod locate;
pub mod loop_mutations;
pub mod milestone_gates;
pub mod mutations;
pub mod next_actions;
pub mod paths;
pub mod peer_mandate;
pub mod plan_skills;
pub mod prompts_plan;
pub mod qa;
mod readback;
mod reconcile_fixes;
pub mod reflection;
pub mod relink;
pub mod reminders;
pub mod resolve;
pub mod retrospective;
pub mod search_index;
pub mod section_links;
pub mod section_queries;
pub mod skill_registry;
pub mod spec;
pub mod spec_render;
pub mod spec_store;
pub mod state;
mod store_index;
mod store_persist;
mod store_queries;
pub mod store_v2;
mod store_watch;
pub mod stride_context;
pub mod stride_log;
pub mod tag_model;
pub mod task_attempts;
pub mod task_queries;
mod task_remove;
mod types;
pub mod usage_digest;
pub mod watcher;
pub mod worker_charter;

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
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timestamp_tests;
#[cfg(test)]
mod watch_reconcile_tests;

use crate::plan::{Plan, PlanMode, PlanStatus, Spec, TaskStatus};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

pub use store_watch::WatchOutcome;
pub use types::{
    IndexEntry, NewTask, OpenSubtask, PlanError, ReconcileFixResult, SpecIndexEntry, TaskLaneUpdate,
};

#[derive(Default)]
pub struct PlanStore {
    plans: RwLock<HashMap<String, Arc<Mutex<Plan>>>>,
    specs: RwLock<HashMap<String, Arc<Mutex<Spec>>>>,
    index: RwLock<HashMap<String, IndexEntry>>,
    spec_index: RwLock<HashMap<String, SpecIndexEntry>>,
    last_writes: RwLock<HashMap<PathBuf, SystemTime>>,
    turn_counter: RwLock<Option<Arc<reminders::TurnCounter>>>,
    reseed_prepare_lock: Mutex<()>,
}

impl PlanStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_for_test(_root: &Path) -> Self {
        #[cfg(test)]
        paths::mark_test_project_root(_root);
        Self::default()
    }
}
