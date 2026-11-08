use super::{PlanError, PlanStore};
use crate::plan::Milestone;
use crate::service_plan::store_db::hot_write::HotChange;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeSet, HashMap};
use std::process::{Command, Output};
use std::sync::Arc;

pub const LOST_COMMIT_STRIDE: &str = "shared_commit_lost";
const REFLOG_SCAN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedSharedCommit {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub sha: String,
}

/// One reported shared commit that the checkout HEAD no longer contains.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LostSharedCommit {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub commit: String,
    pub checkout: String,
    pub old_head: Option<String>,
    pub new_head: String,
    pub cause: String,
}

#[derive(Clone)]
pub struct SharedHistoryPorts {
    /// Delivers one newly recorded incident to the owning Orchestrator.
    pub lost: Arc<dyn Fn(&LostSharedCommit) + Send + Sync>,
    /// Keeps HEAD-change events flowing for a shared checkout.
    pub watch: Arc<dyn Fn(&str) + Send + Sync>,
}

struct VerifiedHead {
    head: String,
    commits: BTreeSet<(String, String)>,
}

#[derive(Default)]
pub(in crate::service_plan) struct SharedHistoryWatch {
    verified: Mutex<HashMap<String, VerifiedHead>>,
    ports: RwLock<Option<SharedHistoryPorts>>,
}

impl PlanStore {
    pub fn bind_shared_history_ports(&self, ports: SharedHistoryPorts) {
        *self.shared_history.ports.write() = Some(ports);
    }

    pub fn shared_history_checkouts(&self) -> Result<Vec<String>, PlanError> {
        self.database.shared_history_checkouts()
    }

    /// Report/dispatch inspection trigger; a detection failure never refuses the caller.
    pub(in crate::service_plan) fn observe_shared_history(&self, checkout: &str, now_ms: u64) {
        let ports = self.shared_history.ports.read().clone();
        if let Some(ports) = ports {
            (ports.watch)(checkout);
        }
        if let Err(error) = self.detect_lost_shared_commits(checkout, now_ms) {
            log::warn!("[shared-history] cannot inspect {checkout}: {error}");
        }
    }

    /// Records one durable incident per reported shared commit missing from HEAD.
    /// A fast-forward from the last verified HEAD costs one ancestry check.
    pub fn detect_lost_shared_commits(
        &self,
        checkout: &str,
        now_ms: u64,
    ) -> Result<Vec<LostSharedCommit>, PlanError> {
        let checkout_key = super::canonical_path(checkout);
        let mut verified = self.shared_history.verified.lock();
        let plan_ids = self.checkout_plan_ids(&checkout_key)?;
        let candidates = if plan_ids.is_empty() {
            Vec::new()
        } else {
            self.database.reported_shared_commits(&plan_ids)?
        };
        if candidates.is_empty() {
            verified.remove(&checkout_key);
            return Ok(Vec::new());
        }
        let head = super::report::shared_checkout_head(checkout).map_err(PlanError::Io)?;
        let previous = verified.remove(&checkout_key);
        let contained = previous
            .as_ref()
            .filter(|previous| {
                previous.head == head || is_ancestor(checkout, &previous.head, &head) == Ok(true)
            })
            .map(|previous| &previous.commits);
        let mut commits = BTreeSet::new();
        let mut lost = Vec::new();
        for candidate in candidates {
            let identity = (candidate.attempt_id.clone(), candidate.sha.clone());
            if contained.is_some_and(|known| known.contains(&identity)) {
                commits.insert(identity);
                continue;
            }
            match is_ancestor(checkout, &candidate.sha, &head) {
                Ok(true) => {
                    commits.insert(identity);
                }
                Ok(false) => lost.push(candidate),
                Err(error) => log::debug!(
                    "[shared-history] cannot check {} in {checkout}: {error}",
                    candidate.sha
                ),
            }
        }
        verified.insert(
            checkout_key,
            VerifiedHead {
                head: head.clone(),
                commits,
            },
        );
        let mut recorded = Vec::new();
        for candidate in lost {
            let stride_id = format!("ms_lost_{}_{}", candidate.attempt_id, candidate.sha);
            if self
                .database
                .stride_recorded(&candidate.plan_id, &stride_id)?
            {
                continue;
            }
            let (old_head, cause) = transition(checkout, &candidate.sha);
            let incident = LostSharedCommit {
                plan_id: candidate.plan_id,
                task_id: candidate.task_id,
                attempt_id: candidate.attempt_id,
                commit: candidate.sha,
                checkout: checkout.to_string(),
                old_head,
                new_head: head.clone(),
                cause: cause.into(),
            };
            let text = serde_json::to_string(&incident)
                .map_err(|error| PlanError::Io(error.to_string()))?;
            self.commit_hot_changes(
                &incident.plan_id,
                vec![HotChange::Stride(Milestone {
                    id: stride_id,
                    from: Default::default(),
                    kind: LOST_COMMIT_STRIDE.into(),
                    text,
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                })],
                now_ms,
            )?;
            recorded.push(incident);
        }
        drop(verified);
        let ports = self.shared_history.ports.read().clone();
        if let Some(ports) = ports {
            for incident in &recorded {
                (ports.lost)(incident);
            }
        }
        Ok(recorded)
    }
}

fn git_output(checkout: &str, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| error.to_string())
}

fn is_ancestor(checkout: &str, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let output = git_output(
        checkout,
        &["merge-base", "--is-ancestor", ancestor, descendant],
    )?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
    }
}

/// The newest reflog step that dropped `commit`: the HEAD before it and its likely cause.
fn transition(checkout: &str, commit: &str) -> (Option<String>, &'static str) {
    let limit = format!("-n{REFLOG_SCAN}");
    let Ok(output) = git_output(
        checkout,
        &["log", "-g", &limit, "--format=%H%x09%gs", "HEAD"],
    ) else {
        return (None, "unknown");
    };
    if !output.status.success() {
        return (None, "unknown");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    for pair in entries.windows(2) {
        let (newer, older) = (pair[0], pair[1]);
        if is_ancestor(checkout, commit, older.0) == Ok(true) {
            return (Some(older.0.to_string()), cause(newer.1));
        }
    }
    (None, "unknown")
}

fn cause(subject: &str) -> &'static str {
    if subject.starts_with("reset:") {
        "reset"
    } else if subject.starts_with("commit (amend)") {
        "amend"
    } else if subject.starts_with("rebase") {
        "rebase"
    } else if subject.starts_with("checkout:") {
        "checkout"
    } else {
        "unknown"
    }
}
