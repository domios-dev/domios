//! Lane cleanup runs only when a lane's fate is decided (acceptance, recorded merge)
//! or its last live holder tab closes; repository ref changes never start lane git work.

use crate::plan::{Plan, Task};
use crate::service::lane_removal::{lane_release_command, LaneRemovalRequest};
use crate::service::plan::TaskLaneUpdate;
use crate::service::ServiceContext;
use codeterm_lanes::checkpoint_io::{run_git_status, CaptureFailure};
use std::path::Path;
use std::time::{Duration, Instant};

pub const CLEANUP_DONE: &str = "done";
pub const CLEANUP_OFFERED: &str = "offered";
pub const CLEANUP_RETAINED: &str = "retained";
pub const CLEANUP_HELD: &str = "held";
const GIT_DEADLINE: Duration = Duration::from_secs(12);
const MAX_REASON_CHARS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ancestry {
    Merged,
    Unmerged,
    BranchGone,
    Unknown(String),
}

pub trait LaneGit {
    fn ancestry(&mut self, canonical: &str, branch: &str, destination: &str) -> Ancestry;
    fn remove(&mut self, request: &LaneRemovalRequest) -> Result<(), String>;
    fn remove_cancelled(&mut self, request: &LaneRemovalRequest) -> Result<(), String> {
        self.remove(request)
    }
    /// The live tab whose cwd is inside the worktree, if any.
    fn holder(&mut self, worktree: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleTrigger {
    Decided,
    HolderClosed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneRecord {
    pub task_id: String,
    pub terminal: bool,
    pub cancelled: bool,
    pub bound_tabs: Vec<String>,
    pub peer: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub destination: Option<String>,
    pub cleanup_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    Removed,
    Record {
        status: &'static str,
        hints: Vec<String>,
        worktree_gone: bool,
        branch_gone: bool,
    },
}

impl LaneRecord {
    pub fn from_task(task: &Task) -> Self {
        Self {
            task_id: task.id.clone(),
            terminal: task.status.is_terminal(),
            cancelled: task.status == crate::plan::TaskStatus::Cancelled,
            bound_tabs: non_empty(task.lane.tab_id.as_deref())
                .into_iter()
                .chain(non_empty(task.assigned_tab_id.as_deref()))
                .chain(
                    task.execution_attempts
                        .iter()
                        .filter_map(|attempt| non_empty(Some(attempt.executor_tab_id.as_str()))),
                )
                .collect(),
            peer: non_empty(task.peer.as_deref()).or_else(|| {
                task.execution_attempts
                    .last()
                    .and_then(|attempt| non_empty(attempt.mesh_peer.as_deref()))
            }),
            worktree: non_empty(task.lane.worktree_path.as_deref()),
            branch: non_empty(task.lane.branch.as_deref()),
            destination: non_empty(task.lane.destination.as_deref()),
            cleanup_status: non_empty(task.lane.cleanup_status.as_deref()),
        }
    }

    /// Decision events settle once; a holder close may revisit retained cleanup debt.
    pub fn needs_settlement(&self) -> bool {
        self.wants(SettleTrigger::Decided)
    }

    /// Closing a recorded holder retries outstanding cleanup debt once.
    pub fn wants(&self, trigger: SettleTrigger) -> bool {
        let open = match self.cleanup_status.as_deref() {
            None => true,
            Some(status) => {
                trigger == SettleTrigger::HolderClosed
                    && [
                        CLEANUP_HELD,
                        CLEANUP_OFFERED,
                        CLEANUP_RETAINED,
                        "cleanup_debt",
                    ]
                    .iter()
                    .any(|value| status.eq_ignore_ascii_case(value))
            }
        };
        self.terminal && open && (self.worktree.is_some() || self.branch.is_some())
    }

    pub fn bound_to(&self, tab_id: &str) -> bool {
        self.bound_tabs.iter().any(|bound| bound == tab_id)
    }
}

pub fn canonical_path(plan: &Plan) -> &str {
    if plan.canonical_project_path.trim().is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    }
}

/// One ancestry check, then the safe removal only when the lane is merged.
pub fn settle(
    record: &LaneRecord,
    canonical: &str,
    worktree_exists: bool,
    git: &mut dyn LaneGit,
) -> Settlement {
    if let Some(peer) = &record.peer {
        return offered(vec![format!(
            "lane lives on peer {peer}; clean it on that peer"
        )]);
    }
    let release = lane_release_command(
        record.worktree.as_deref(),
        record.branch.as_deref(),
        canonical,
    );
    if let Some(holder) = record
        .worktree
        .as_deref()
        .and_then(|worktree| git.holder(worktree))
    {
        return Settlement::Record {
            status: CLEANUP_HELD,
            hints: std::iter::once(format!(
                "worktree is open in live tab {holder}; settled again when it closes"
            ))
            .chain(release.clone())
            .collect(),
            worktree_gone: false,
            branch_gone: false,
        };
    }
    let ancestry = match record.branch.as_deref() {
        Some(branch) => git.ancestry(canonical, branch, "HEAD"),
        None => Ancestry::BranchGone,
    };
    let retained = |reason: String| Settlement::Record {
        status: CLEANUP_RETAINED,
        hints: std::iter::once(reason).chain(release.clone()).collect(),
        worktree_gone: false,
        branch_gone: false,
    };
    if record.cancelled && ancestry == Ancestry::Unmerged {
        let Some(worktree) = record.worktree.as_deref().filter(|_| worktree_exists) else {
            return retained("cancelled lane branch retained for recovery".into());
        };
        let request = LaneRemovalRequest {
            worktree_path: worktree.into(),
            branch: None,
            force: false,
            canonical_cwd: Some(canonical.into()),
        };
        return match git.remove_cancelled(&request) {
            Ok(()) => Settlement::Record {
                status: CLEANUP_RETAINED,
                hints: std::iter::once(format!(
                    "cancelled clean worktree removed; branch {} retained for recovery",
                    record.branch.as_deref().unwrap_or_default()
                ))
                .chain(lane_release_command(
                    None,
                    record.branch.as_deref(),
                    canonical,
                ))
                .collect(),
                worktree_gone: true,
                branch_gone: false,
            },
            Err(reason) => retained(reason),
        };
    }
    match ancestry {
        Ancestry::Unmerged => retained(format!(
            "branch {} has commits not merged into {}; lane kept",
            record.branch.as_deref().unwrap_or_default(),
            "HEAD",
        )),
        Ancestry::Unknown(reason) => {
            retained(bounded(format!("ancestry unknown: {reason}; lane kept")))
        }
        Ancestry::BranchGone if !worktree_exists => Settlement::Record {
            status: CLEANUP_DONE,
            hints: Vec::new(),
            worktree_gone: true,
            branch_gone: true,
        },
        Ancestry::Merged | Ancestry::BranchGone => {
            let Some(worktree) = record.worktree.clone() else {
                return offered(release.clone().into_iter().collect());
            };
            let request = LaneRemovalRequest {
                worktree_path: worktree,
                branch: record.branch.clone(),
                force: false,
                canonical_cwd: Some(canonical.to_string()),
            };
            match git.remove(&request) {
                Ok(()) => Settlement::Removed,
                Err(reason) => retained(reason),
            }
        }
    }
}

/// Startup pass over history: no git, only whether each recorded worktree still exists.
pub fn settle_history(
    records: &[LaneRecord],
    canonical: &str,
    worktree_exists: &dyn Fn(&str) -> bool,
) -> Vec<(String, Settlement)> {
    records
        .iter()
        .filter(|record| record.needs_settlement())
        .map(|record| {
            let settlement = if let Some(peer) = &record.peer {
                offered(vec![format!(
                    "lane lives on peer {peer}; clean it on that peer"
                )])
            } else if record
                .worktree
                .as_deref()
                .is_some_and(|path| worktree_exists(path))
            {
                offered(
                    lane_release_command(
                        record.worktree.as_deref(),
                        record.branch.as_deref(),
                        canonical,
                    )
                    .into_iter()
                    .collect(),
                )
            } else {
                Settlement::Record {
                    status: CLEANUP_DONE,
                    hints: Vec::new(),
                    worktree_gone: true,
                    branch_gone: false,
                }
            };
            (record.task_id.clone(), settlement)
        })
        .collect()
}

pub fn lane_update(settlement: &Settlement) -> Option<TaskLaneUpdate> {
    let Settlement::Record {
        status,
        hints,
        worktree_gone,
        branch_gone,
    } = settlement
    else {
        return None;
    };
    let cleared = |gone: bool| gone.then_some(None);
    Some(TaskLaneUpdate {
        tab_id: cleared(*worktree_gone),
        workspace_id: cleared(*worktree_gone),
        workspace_name: cleared(*worktree_gone),
        worktree_path: cleared(*worktree_gone),
        branch: cleared(*branch_gone),
        cleanup_status: Some(Some((*status).to_string())),
        cleanup_hints: Some(hints.clone()),
        ..Default::default()
    })
}

fn apply_to_task(task: &mut Task, update: TaskLaneUpdate) {
    let lane = &mut task.lane;
    for (field, value) in [
        (&mut lane.tab_id, update.tab_id),
        (&mut lane.workspace_id, update.workspace_id),
        (&mut lane.workspace_name, update.workspace_name),
        (&mut lane.worktree_path, update.worktree_path),
        (&mut lane.branch, update.branch),
        (&mut lane.cleanup_status, update.cleanup_status),
    ] {
        if let Some(value) = value {
            *field = value;
        }
    }
    if let Some(hints) = update.cleanup_hints {
        lane.cleanup_hints = hints;
    }
}

fn offered(hints: Vec<String>) -> Settlement {
    Settlement::Record {
        status: CLEANUP_OFFERED,
        hints,
        worktree_gone: false,
        branch_gone: false,
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bounded(reason: String) -> String {
    match reason.char_indices().nth(MAX_REASON_CHARS) {
        Some((cut, _)) => format!("{}…", &reason[..cut]),
        None => reason,
    }
}

fn worktree_exists(path: &str) -> bool {
    std::fs::symlink_metadata(codeterm_paths::strip_verbatim_prefix(path)).is_ok()
}

struct ServiceLaneGit<'a> {
    service: &'a ServiceContext,
    spawned: usize,
}

impl LaneGit for ServiceLaneGit<'_> {
    fn ancestry(&mut self, canonical: &str, branch: &str, destination: &str) -> Ancestry {
        let (ancestry, spawned) = git_ancestry(canonical, branch, destination);
        self.spawned += spawned;
        ancestry
    }

    fn remove(&mut self, request: &LaneRemovalRequest) -> Result<(), String> {
        self.service.remove_owned_lane(request).map(|_| ())
    }

    fn remove_cancelled(&mut self, request: &LaneRemovalRequest) -> Result<(), String> {
        let outcome = crate::service::lane_removal::remove_cancelled_worktree(
            request,
            &self.service.live_tab_cwds(),
        )?;
        crate::service::lane_removal::finalize_lane_removal(
            &self.service.plan_store,
            self.service
                .ext::<crate::repowatch::RepoWatchRegistryHandle>(),
            &outcome,
        )
    }

    fn holder(&mut self, worktree: &str) -> Option<String> {
        let tabs = self.service.live_tab_cwds();
        crate::service::lane_removal::find_lane_holder(worktree, &tabs)
            .map(|tab| format!("'{}' ({})", tab.title, tab.id))
    }
}

/// One `merge-base` call; only its error path asks whether the branch still exists.
fn git_ancestry(canonical: &str, branch: &str, destination: &str) -> (Ancestry, usize) {
    let status = match run_git_status(
        Path::new(canonical),
        &["merge-base", "--is-ancestor", branch, destination],
        Instant::now() + GIT_DEADLINE,
    ) {
        Ok(output) => output,
        Err(failure) => return (Ancestry::Unknown(failure_text(failure)), 1),
    };
    match status.status.code() {
        Some(0) => return (Ancestry::Merged, 1),
        Some(1) => return (Ancestry::Unmerged, 1),
        _ => {}
    }
    let stderr = String::from_utf8_lossy(&status.stderr).trim().to_string();
    let reference = if branch.starts_with("refs/") {
        branch.to_string()
    } else {
        format!("refs/heads/{branch}")
    };
    let ancestry = match run_git_status(
        Path::new(canonical),
        &["show-ref", "--verify", "--quiet", &reference],
        Instant::now() + GIT_DEADLINE,
    ) {
        Ok(output) if output.status.code() == Some(1) => Ancestry::BranchGone,
        _ => Ancestry::Unknown(stderr),
    };
    (ancestry, 2)
}

fn failure_text(failure: CaptureFailure) -> String {
    match failure {
        CaptureFailure::Deadline => "git exceeded its deadline".into(),
        CaptureFailure::Git(error) => error,
    }
}

impl ServiceContext {
    fn lane_git_queue(&self) -> &crate::service::lane_git_queue::LaneGitQueue {
        self.extensions
            .get_or_init(crate::service::lane_git_queue::LaneGitQueue::background)
    }

    pub(crate) fn enqueue_lane_git(
        &self,
        repository: &str,
        key: String,
        job: crate::service::lane_git_queue::LaneGitJob,
    ) {
        self.lane_git_queue().enqueue(repository, key, job);
    }

    /// Settle the lane of a task whose fate was just decided; queued, never inline.
    pub fn settle_lane(&self, plan_id: &str, task_id: &str) {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return;
        };
        if plan.tasks.iter().any(|task| {
            task.id == task_id && LaneRecord::from_task(task).wants(SettleTrigger::Decided)
        }) {
            self.enqueue_settlement(&plan, task_id, SettleTrigger::Decided);
        }
    }

    /// A closed tab may have been the only thing keeping a finished lane in place.
    pub fn settle_lanes_held_by_closed_tab(&self, tab_id: &str) {
        for plan in self.plan_store.list_full() {
            for task_id in held_by_tab(&plan, tab_id) {
                self.enqueue_settlement(&plan, &task_id, SettleTrigger::HolderClosed);
            }
        }
    }

    fn enqueue_settlement(&self, plan: &Plan, task_id: &str, trigger: SettleTrigger) {
        let service = self.clone();
        let (plan_id, task_id) = (plan.id.clone(), task_id.to_string());
        self.enqueue_lane_git(
            canonical_path(plan),
            format!("settle:{plan_id}:{task_id}"),
            Box::new(move || service.run_settlement(&plan_id, &task_id, trigger)),
        );
    }

    fn run_settlement(&self, plan_id: &str, task_id: &str, trigger: SettleTrigger) {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return;
        };
        let Some(record) = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .map(LaneRecord::from_task)
            .filter(|record| record.wants(trigger))
        else {
            return;
        };
        let canonical = canonical_path(&plan);
        let exists = record.worktree.as_deref().is_some_and(worktree_exists);
        let mut git = ServiceLaneGit {
            service: self,
            spawned: 0,
        };
        let bound_live = self
            .mux
            .read()
            .all_tabs()
            .find(|tab| record.bound_to(&tab.id))
            .map(|tab| tab.id.clone());
        let settlement = if let Some(tab) = bound_live.filter(|_| record.peer.is_none()) {
            Settlement::Record {
                status: CLEANUP_HELD,
                hints: std::iter::once(format!(
                    "Worker tab {tab} is live; cleanup runs when it closes"
                ))
                .chain(lane_release_command(
                    record.worktree.as_deref(),
                    record.branch.as_deref(),
                    canonical,
                ))
                .collect(),
                worktree_gone: false,
                branch_gone: false,
            }
        } else {
            settle(&record, canonical, exists, &mut git)
        };
        log::debug!(
            "[lane-settle] {plan_id}/{task_id}: {settlement:?} ancestry_git_spawns={}",
            git.spawned
        );
        if let Some(update) = lane_update(&settlement) {
            if let Err(error) =
                self.plan_store
                    .update_task_lane(plan_id, task_id, update, unix_time_millis())
            {
                log::warn!("[lane-settle] {plan_id}/{task_id} could not be recorded: {error}");
                return;
            }
        }
        self.ui
            .emit("plan-changed", serde_json::json!({ "plan_id": plan_id }));
    }

    /// Marks every unsettled lane of a finished task terminal so history is never re-checked.
    pub fn settle_finished_lane_history(&self) {
        let now = unix_time_millis();
        for plan_id in self.plan_store.plan_ids_with_unsettled_lanes() {
            if !self
                .plan_store
                .plan_matches(&plan_id, |plan| !finished_records(plan).is_empty())
            {
                continue;
            }
            let result = self.plan_store.with_plan(&plan_id, now, |plan| {
                let canonical = canonical_path(plan).to_string();
                let settlements =
                    settle_history(&finished_records(plan), &canonical, &worktree_exists);
                for (task_id, settlement) in &settlements {
                    let task = plan.tasks.iter_mut().find(|task| task.id == *task_id);
                    if let (Some(task), Some(update)) = (task, lane_update(settlement)) {
                        apply_to_task(task, update);
                    }
                }
                Ok(settlements.len())
            });
            match result {
                Ok(count) => {
                    log::debug!("[lane-settle] {plan_id}: {count} finished lanes marked");
                    self.ui
                        .emit("plan-changed", serde_json::json!({ "plan_id": plan_id }));
                }
                Err(error) => log::warn!("[lane-settle] {plan_id} history not marked: {error}"),
            }
        }
    }
}

pub fn held_by_tab(plan: &Plan, tab_id: &str) -> Vec<String> {
    plan.tasks
        .iter()
        .map(LaneRecord::from_task)
        .filter(|record| record.bound_to(tab_id) && record.wants(SettleTrigger::HolderClosed))
        .map(|record| record.task_id)
        .collect()
}

fn finished_records(plan: &Plan) -> Vec<LaneRecord> {
    plan.tasks
        .iter()
        .map(LaneRecord::from_task)
        .filter(LaneRecord::needs_settlement)
        .collect()
}

fn unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
