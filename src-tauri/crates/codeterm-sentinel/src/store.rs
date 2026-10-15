use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{atomic::Ordering, Arc};

use serde::{Deserialize, Serialize};

use super::amend::{self, Amendment};
use super::escalation;
use super::handlers::{self, Definition, Handler, Transition};
use super::machine::{self, Machine, Route};
use super::probe::cancellation::Cancellation;
use super::source::{self, RunBinding, TabBinding};
use super::storage::{Backend, LoadFailure, Staged};
use codeterm_blueprint_types::sentinel_input::InputSpec;
use codeterm_cron::clock::SharedClock;

pub const MAX_SENTINELS: usize = 256;
pub const MAX_ACTIVE: usize = 64;
pub const MAX_LIFETIME_TRANSITIONS: usize = 65_536;

pub type ScopeKey = (String, Option<String>);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    #[serde(default)]
    pub terminal: Option<codeterm_blueprint_types::blueprint_terminal::Config>,
    #[serde(default)]
    pub session: bool,
    pub name: Option<String>,
    #[serde(default)]
    pub probe: Option<String>,
    #[serde(default)]
    pub observe: Option<TabBinding>,
    #[serde(default)]
    pub input: Option<InputSpec>,
    pub question: String,
    pub buckets: Vec<String>,
    #[serde(default = "default_every")]
    pub every: String,
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
    #[serde(default)]
    pub model_classification: bool,
    #[serde(default)]
    pub rearm_on_change: BTreeSet<String>,
    pub until: Option<String>,
    pub timeout: Option<String>,
    #[serde(default)]
    pub paused: bool,
    pub handlers: Option<BTreeMap<String, Vec<Definition>>>,
    #[serde(default)]
    pub escalation_timeout_ms: Option<u64>,
    pub max_transitions: Option<u16>,
    pub max_actions: Option<u16>,
}

fn default_every() -> String {
    "20s".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sentinel {
    #[serde(flatten)]
    pub source: super::stage_source::StageSource,
    #[serde(default)]
    pub session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causal_run: Option<codeterm_blueprint_types::blueprint_run::Context>,
    pub id: String,
    pub team_id: Option<String>,
    pub creator_tab_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub question: String,
    pub buckets: Vec<String>,
    pub interval_ms: u64,
    pub routes: BTreeMap<String, Vec<Handler>>,
    pub matches: BTreeMap<String, String>,
    #[serde(default)]
    pub model_classification: bool,
    #[serde(default)]
    pub rearm_on_change: BTreeSet<String>,
    pub until: Option<String>,
    pub expires_at: Option<u64>,
    pub next_run: u64,
    pub machine: Machine,
    pub last_reason: Option<String>,
    #[serde(default, flatten)]
    pub progress: super::progress::Progress,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub agent_authored: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub scenario_id: Option<String>,
    #[serde(default = "handlers::default_transitions")]
    pub max_transitions: u16,
    #[serde(default = "handlers::default_actions")]
    pub max_actions: u16,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub run_transitions: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_other: Vec<Handler>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_timeout_ms: Option<u64>,
    #[serde(
        default = "amend::initial_version",
        skip_serializing_if = "amend::is_initial_version"
    )]
    pub definition_version: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub amendments: Vec<Amendment>,
}

#[derive(Clone)]
pub struct Owner {
    pub creator_tab_id: String,
    pub team_id: Option<String>,
    pub workspace_id: String,
    pub cwd: String,
}

pub struct Lease {
    pub job: Sentinel,
    pub cancelled: Arc<Cancellation>,
    pub event: Option<codeterm_plugin::graph_events::EventEnvelope>,
}

#[cfg(any(test, feature = "test-support"))]
impl Lease {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub struct Store {
    pub jobs: Vec<Sentinel>,
    pub active: HashMap<String, Arc<Cancellation>>,
    pub wake_pending: BTreeSet<String>,
    pub backend: Backend,
    pub clock: SharedClock,
    pub unavailable: Option<LoadFailure>,
    pub admission_closed: bool,
    pub committed_revisions: BTreeMap<ScopeKey, String>,
    pub changes: BTreeMap<ScopeKey, String>,
    pub pending_stop_notices: Vec<StopNotice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopNotice {
    pub sentinel_id: String,
    pub name: String,
    pub creator_tab_id: String,
    pub reason: String,
}

impl StopNotice {
    pub fn new(job: &Sentinel, reason: &str) -> Self {
        Self {
            sentinel_id: job.id.clone(),
            name: job.name.clone().unwrap_or_else(|| job.id.clone()),
            creator_tab_id: job.creator_tab_id.clone(),
            reason: reason.into(),
        }
    }
}

/// Operational scheduling changes staged by `due`; never authored definition versions.
pub enum ScheduleChange {
    Advance { id: String, next_run: u64 },
    Stop { id: String, reason: &'static str },
}

impl Store {
    pub fn cancel_tab_owned_runs(&mut self, tab_id: &str) -> usize {
        let active = self
            .jobs
            .iter()
            .filter(|job| job.creator_tab_id == tab_id && job.team_id.is_none())
            .filter_map(|job| self.active.get(&job.id))
            .cloned()
            .collect::<Vec<_>>();
        let count = active.len();
        for cancellation in active {
            cancellation.cancel();
        }
        count
    }

    pub fn stage_event_lease(
        &mut self,
        event: codeterm_plugin::graph_events::EventEnvelope,
    ) -> Option<Lease> {
        if self.admission_closed || self.ensure_available().is_err() {
            return None;
        }
        let now = self.clock.now_ms();
        let job = self.jobs.iter().find(|job| job.id == event.pipeline_id)?;
        let matches_kind = job.pipeline().is_some_and(|pipeline| {
            matches!(&pipeline.trigger, codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { kind, .. } if kind == &event.kind)
        });
        if !matches_kind
            || job.machine.stopped
            || job.machine.paused
            || job.machine.held.is_some()
            || job.expires_at.is_some_and(|at| now >= at)
            || self.active.contains_key(&job.id)
        {
            return None;
        }
        let job = job.clone();
        let cancelled = Arc::new(Cancellation::new(false));
        self.active.insert(job.id.clone(), cancelled.clone());
        Some(Lease {
            job,
            cancelled,
            event: Some(event),
        })
    }

    pub fn committed_revision(&self, workspace: &str, team: Option<&str>) -> Option<&str> {
        self.committed_revisions
            .get(&(workspace.into(), team.map(str::to_owned)))
            .map(String::as_str)
    }

    pub fn take_changes(&mut self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.changes).into_iter().map(|((workspace_id,team_id),revision)| serde_json::json!({"workspace_id":workspace_id,"team_id":team_id,"revision":revision})).collect()
    }

    pub fn take_stop_notices(&mut self) -> Vec<StopNotice> {
        std::mem::take(&mut self.pending_stop_notices)
    }

    pub fn ensure_available(&self) -> Result<(), String> {
        if let Some(failure) = self.unavailable {
            return Err(failure.message());
        }
        if self.backend.held() {
            return Err("blueprint_store_load_failed: persistence is held for repair".into());
        }
        Ok(())
    }

    pub fn list(&self) -> &[Sentinel] {
        &self.jobs
    }
    pub fn get(&self, id: &str) -> Option<&Sentinel> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn stage_insert(&mut self, job: Sentinel) -> Staged<Sentinel> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        if let Err(error) = escalation::validate(&job) {
            return Staged::ready(Err(error));
        }
        if let Some(capability) = job.pending_capability() {
            return Staged::ready(Err(format!(
                "blueprint_capability_unavailable: {capability} is not executable yet"
            )));
        }
        if self.jobs.len() >= MAX_SENTINELS
            || (!job.machine.paused && self.active_count() >= MAX_ACTIVE)
        {
            return Staged::ready(Err("blueprint capacity reached".into()));
        }
        if self.get(&job.id).is_some() {
            return Staged::ready(Err("duplicate blueprint ID".into()));
        }
        let association_ids = match self.association_ids(&job) {
            Ok(ids) => ids,
            Err(error) => return Staged::ready(Err(error)),
        };
        if !self.backend.is_json() {
            return self.sqlite_insert(job, &association_ids);
        }
        let previous = self.jobs.clone();
        for child in self
            .jobs
            .iter_mut()
            .filter(|child| association_ids.contains(&child.id))
        {
            child.scenario_id = job.scenario_id.clone();
        }
        self.jobs.push(job.clone());
        if let Err(error) = self.save_legacy() {
            self.jobs = previous;
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(job))
    }

    /// Validates exact targets and collects unassociated stages joining the new scenario.
    pub fn association_ids(&self, job: &Sentinel) -> Result<Vec<String>, String> {
        self.validate_targets(job)?;
        let mut association_ids = Vec::new();
        if job.scenario_id.is_some() {
            let mut pending: Vec<String> = job
                .routes
                .values()
                .flatten()
                .filter_map(|h| {
                    if let Route::Sentinel(id) = &h.action {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();
            let mut seen = std::collections::BTreeSet::new();
            while let Some(id) = pending.pop() {
                if !seen.insert(id.clone()) {
                    continue;
                }
                self.validate_target(job, &id)?;
                let child = self.get(&id).ok_or("scenario target disappeared")?;
                if child.scenario_id.is_none() {
                    association_ids.push(id.clone());
                    pending.extend(child.routes.values().flatten().filter_map(|h| {
                        if let Route::Sentinel(id) = &h.action {
                            Some(id.clone())
                        } else {
                            None
                        }
                    }));
                }
            }
        }
        Ok(association_ids)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn insert(&mut self, job: Sentinel) -> Result<Sentinel, String> {
        self.locked(|store| store.stage_insert(job))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn create(
        &mut self,
        params: Create,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let job = self.prepare(params, owner, connected)?;
        self.insert(job)
    }

    pub fn close_admission(&mut self) {
        self.admission_closed = true;
        for cancellation in self.active.values() {
            cancellation.cancel();
        }
    }

    pub fn stage_stop(&mut self, id: &str, reason: &str) -> Staged<Sentinel> {
        let Some(current) = self.get(id).cloned() else {
            return Staged::ready(Err(format!("blueprint not found: {id}")));
        };
        let notice = (!current.machine.stopped).then(|| StopNotice::new(&current, reason));
        if let Some(cancelled) = self.active.get(id) {
            cancelled.store(true, Ordering::SeqCst);
        }
        if !self.backend.is_json() {
            return self.sqlite_stop(id, reason, notice);
        }
        let Some(stopped) = self.apply_stop(id, reason) else {
            return Staged::ready(Err(format!("blueprint not found: {id}")));
        };
        match self.save_legacy() {
            Ok(()) => {
                if let Some(notice) = notice {
                    self.pending_stop_notices.push(notice);
                }
                Staged::ready(Ok(stopped))
            }
            Err(error) => Staged::ready(Err(error)),
        }
    }

    /// The in-memory stop; callers cancel the live lease themselves.
    pub fn apply_stop(&mut self, id: &str, reason: &str) -> Option<Sentinel> {
        let job = self.jobs.iter_mut().find(|job| job.id == id)?;
        job.machine.stopped = true;
        job.revision += 1;
        job.last_reason = Some(reason.into());
        self.wake_pending.remove(id);
        Some(job.clone())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn stop(&mut self, id: &str, reason: &str) -> Result<Sentinel, String> {
        self.locked(|store| store.stage_stop(id, reason))
    }

    pub fn stage_remove_team(&mut self, team: &str) -> Staged<()> {
        for job in self
            .jobs
            .iter()
            .filter(|job| job.team_id.as_deref() == Some(team))
        {
            if let Some(cancelled) = self.active.remove(&job.id) {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
        if !self.backend.is_json() {
            return self.sqlite_remove_team(team);
        }
        let removed: BTreeSet<_> = self
            .jobs
            .iter()
            .filter(|job| job.team_id.as_deref() == Some(team))
            .map(|job| job.id.clone())
            .collect();
        self.jobs.retain(|job| job.team_id.as_deref() != Some(team));
        self.wake_pending.retain(|id| !removed.contains(id));
        Staged::ready(self.save_legacy())
    }

    pub fn stage_delete_scenario(
        &mut self,
        id: &str,
        workspace: &str,
        team: Option<&str>,
    ) -> Staged<()> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let owned: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| {
                job.workspace_id == workspace
                    && job.team_id.as_deref() == team
                    && (job.id == id || job.scenario_id.as_deref() == Some(id))
            })
            .map(|job| job.id.clone())
            .collect();
        if !owned.iter().any(|candidate| candidate == id) {
            return Staged::ready(Err(format!("Blueprint scenario not found: {id}")));
        }
        for job_id in &owned {
            if let Some(cancelled) = self.active.remove(job_id) {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
        if !self.backend.is_json() {
            return self.sqlite_delete_scenario(id, workspace, team);
        }
        let owned: BTreeSet<_> = owned.into_iter().collect();
        let previous = self.jobs.clone();
        self.jobs.retain(|job| !owned.contains(&job.id));
        self.wake_pending.retain(|job_id| !owned.contains(job_id));
        let result = self.save_legacy();
        if result.is_err() {
            self.jobs = previous;
        }
        Staged::ready(result)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn remove_team(&mut self, team: &str) -> Result<(), String> {
        self.locked(|store| store.stage_remove_team(team))
    }

    /// The typed backend plans on a copy, so the cache changes only on publication.
    pub fn stage_due(&mut self) -> Staged<Vec<Lease>> {
        if self.admission_closed {
            return Staged::ready(Err("blueprint_shutdown_admission_closed".into()));
        }
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let now = self.clock.now_ms();
        if !self.backend.is_json() {
            let mut planned = self.jobs.clone();
            let (due, changes, consumed_wakes) =
                plan_due(&mut planned, &mut self.active, &mut self.wake_pending, now);
            let notices = scheduled_stop_notices(&planned, &changes);
            return self.sqlite_due(due, changes, consumed_wakes, notices);
        }
        let previous = self.jobs.clone();
        let previous_wakes = self.wake_pending.clone();
        let (due, changes, _) = plan_due(
            &mut self.jobs,
            &mut self.active,
            &mut self.wake_pending,
            now,
        );
        if !changes.is_empty() {
            if let Err(error) = self.save_legacy() {
                for lease in &due {
                    lease.cancelled.store(true, Ordering::SeqCst);
                    self.active.remove(&lease.job.id);
                }
                self.jobs = previous;
                self.wake_pending = previous_wakes;
                return Staged::ready(Err(error));
            }
        }
        self.pending_stop_notices
            .extend(scheduled_stop_notices(&self.jobs, &changes));
        Staged::ready(Ok(due))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn due(&mut self) -> Result<Vec<Lease>, String> {
        self.locked(Store::stage_due)
    }

    pub fn stage_finish(&mut self, job: &Sentinel) -> Staged<bool> {
        self.stage_finish_with(job, None)
    }

    pub fn stage_resolve_escalation(&mut self, id: &str, transition_id: &str) -> Staged<bool> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(current) = self.get(id).cloned() else {
            return Staged::ready(Err(format!("blueprint not found: {id}")));
        };
        if current.machine.held.as_deref() != Some(transition_id) {
            return Staged::ready(Err(
                "blueprint escalation is not open for that transition".into()
            ));
        }
        if let Some(cancelled) = self.active.remove(id) {
            cancelled.cancel();
        }
        let mut resolved = current;
        resolved.machine.held = None;
        resolved.machine.last_hash = None;
        resolved.last_reason = Some("escalation_resolved".into());
        self.stage_finish_with(&resolved, None)
    }

    pub fn wake_stage(&mut self, id: &str) -> bool {
        let Some(job) = self.get(id) else {
            return false;
        };
        if job.machine.stopped || job.machine.paused || job.machine.held.is_some() {
            return false;
        }
        if job.pipeline().is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
            )
        }) {
            return false;
        }
        self.wake_pending.insert(id.into())
    }

    pub fn stage_expire_escalation(&mut self) -> Option<(Sentinel, Staged<bool>)> {
        let now = self.clock.now_ms();
        let current = self
            .jobs
            .iter()
            .find(|job| {
                !job.machine.stopped
                    && !self.active.contains_key(&job.id)
                    && !job.expires_at.is_some_and(|at| now >= at)
                    && super::escalation::expiry_at(job).is_some_and(|at| now >= at)
            })?
            .clone();
        let mut expired = current.clone();
        expired.machine.held = None;
        expired.last_reason = Some("escalation_expired".into());
        Some((current, self.stage_finish_with(&expired, None)))
    }

    /// Records a lease outcome; with `transition` it also accepts that transition.
    pub fn stage_finish_with(
        &mut self,
        job: &Sentinel,
        transition: Option<Transition>,
    ) -> Staged<bool> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let now = self.clock.now_ms();
        let Some(current) = self.get(&job.id) else {
            return Staged::ready(Ok(false));
        };
        if !current.same_owner(job)
            || current.revision != job.revision
            || current.machine.stopped
            || current.expires_at.is_some_and(|at| now >= at)
        {
            return Staged::ready(Ok(false));
        }
        if !self.backend.is_json() {
            return self.sqlite_finish(job, transition);
        }
        if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
            apply_finish(current, job, transition);
        }
        if let Err(error) = self.save_legacy() {
            if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
                current.machine.stopped = true;
                current.last_reason = Some("persistence_failed".into());
            }
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(true))
    }

    /// A failed commit keeps the outcome in memory but stops the stage, as the file did.
    pub fn keep_finish_intent(&mut self, job: &Sentinel, transition: Option<Transition>) {
        if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
            apply_finish(current, job, transition);
            current.machine.stopped = true;
            current.last_reason = Some("persistence_failed".into());
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn finish(&mut self, job: &Sentinel) -> Result<bool, String> {
        self.locked(|store| store.stage_finish(job))
    }

    fn active_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|job| !job.machine.stopped && !job.machine.paused)
            .count()
    }

    pub fn validate_target(&self, source: &Sentinel, id: &str) -> Result<(), String> {
        self.ensure_available()?;
        if source.id == id {
            return Err("blueprint cannot activate itself".into());
        }
        let target = self.get(id).ok_or("blueprint target not found")?;
        if !source.same_owner(target) {
            return Err("blueprint target requires the same creator, Team and workspace".into());
        }
        if target.machine.stopped
            || target
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
        {
            return Err("blueprint target is stopped or expired".into());
        }
        Ok(())
    }

    pub fn validate_targets(&self, source: &Sentinel) -> Result<(), String> {
        for handler in source.routes.values().flatten() {
            if let Route::Sentinel(id) = &handler.action {
                self.validate_target(source, id)?;
            }
        }
        Ok(())
    }

    pub fn stage_activate(
        &mut self,
        source: &Sentinel,
        transition: &str,
        id: &str,
    ) -> Staged<bool> {
        match self.activation_check(source, transition, id) {
            Ok(true) => {}
            Ok(false) => return Staged::ready(Ok(false)),
            Err(error) => return Staged::ready(Err(error)),
        }
        let now = self.clock.now_ms();
        let input_activation = self.get(id).is_some_and(|target| {
            target.pipeline().is_some_and(|pipeline| {
                matches!(
                    &pipeline.trigger,
                    codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
                )
            })
        });
        if !self.backend.is_json() {
            return self.sqlite_activate(source, transition, id, input_activation.then_some(now));
        }
        let causal_run = self.get(&source.id).and_then(|current| {
            let record = current
                .transitions
                .iter()
                .find(|record| record.id == transition)?;
            let handler = current.routes.get(&record.bucket)?.iter().find(|handler| {
                matches!(&handler.action, Route::Sentinel(target) if target == id)
                    && record.handlers.iter().any(|claim| {
                        claim.handler_id == handler.id
                            && claim.claim_id.is_some()
                            && claim.status == handlers::Status::Unconfirmed
                    })
            })?;
            record
                .causal_run
                .as_ref()
                .map(|run| run.child(&source.id, transition, &handler.id))
        });
        let previous = self.jobs.clone();
        let Some(target) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return Staged::ready(Err("blueprint target disappeared".into()));
        };
        target.causal_run = causal_run;
        target.machine.paused = false;
        if input_activation {
            target.next_run = now;
        }
        target.revision += 1;
        if let Err(error) = self.save_legacy() {
            self.jobs = previous;
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(true))
    }

    /// `Ok(false)` when the target already runs; every error fences the activation.
    fn activation_check(
        &self,
        source: &Sentinel,
        transition: &str,
        id: &str,
    ) -> Result<bool, String> {
        if !self.action_permitted(source, transition) {
            return Err("blueprint activation source fenced".into());
        }
        let current = self
            .get(&source.id)
            .ok_or("blueprint activation source disappeared")?;
        let transition = current
            .transitions
            .iter()
            .find(|record| record.id == transition)
            .ok_or("blueprint activation transition disappeared")?;
        let declared_claim = current
            .routes
            .get(&transition.bucket)
            .is_some_and(|handlers| {
                handlers.iter().any(|handler| {
                    matches!(&handler.action, Route::Sentinel(target) if target == id)
                        && transition.handlers.iter().any(|claim| {
                            claim.handler_id == handler.id
                                && claim.claim_id.is_some()
                                && claim.status == handlers::Status::Unconfirmed
                        })
                })
            });
        if !declared_claim {
            return Err("blueprint activation requires its declared durable handler claim".into());
        }
        self.validate_target(source, id)?;
        if !self
            .get(id)
            .ok_or("blueprint target disappeared")?
            .machine
            .paused
        {
            return Ok(false);
        }
        if self.active_count() >= MAX_ACTIVE {
            return Err("blueprint active capacity reached".into());
        }
        Ok(true)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn activate(
        &mut self,
        source: &Sentinel,
        transition: &str,
        id: &str,
    ) -> Result<bool, String> {
        self.locked(|store| store.stage_activate(source, transition, id))
    }

    pub fn has_live_lease(&self, id: &str) -> bool {
        self.active
            .get(id)
            .is_some_and(|cancelled| !cancelled.load(Ordering::SeqCst))
    }

    pub fn live_leases(&self) -> std::collections::BTreeSet<String> {
        self.active
            .keys()
            .filter(|id| self.has_live_lease(id))
            .cloned()
            .collect()
    }

    pub fn release(&mut self, id: &str) {
        if self.active.remove(id).is_some() && self.ensure_available().is_ok() {
            if let Some(job) = self.get(id).filter(|job| job.machine.stopped) {
                let scope = (job.workspace_id.clone(), job.team_id.clone());
                if let Some(revision) = self.committed_revisions.get(&scope) {
                    self.changes.insert(scope, revision.clone());
                }
            }
        }
    }

    pub fn wake_tab(&mut self, tab_id: &str) -> bool {
        if !source::bounded_id(tab_id) {
            return false;
        }
        let now = self.clock.now_ms();
        let matching: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| {
                !job.machine.stopped
                    && job.terminal().is_none()
                    && !job.machine.paused
                    && !job.expires_at.is_some_and(|at| now >= at)
                    && (matches!(job.observe(), Some(TabBinding::Tab(id)) if id == tab_id)
                        || matches!(job.observe(), Some(TabBinding::CronRun(_))
                            if job.run.as_ref().is_some_and(|run| run.tab_id == tab_id)))
            })
            .map(|job| job.id.clone())
            .collect();
        let mut changed = false;
        for id in matching {
            changed |= self.wake_pending.insert(id);
        }
        changed
    }

    pub fn next_deadline(&self) -> u64 {
        let now = self.clock.now_ms();
        self.jobs
            .iter()
            .filter(|job| !job.machine.stopped)
            .flat_map(|job| {
                let active = self.active.contains_key(&job.id);
                let waiting_for_run =
                    matches!(job.observe(), Some(TabBinding::CronRun(_))) && job.run.is_none();
                let event_trigger = job.pipeline().is_some_and(|pipeline| {
                    matches!(
                        &pipeline.trigger,
                        codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
                    )
                });
                let input_waiting = job.pipeline().is_some_and(|pipeline| {
                    matches!(
                        &pipeline.trigger,
                        codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
                    ) && job.next_run
                        == codeterm_blueprint_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS
                });
                let next = if ((job.terminal().is_some()
                    || (job.input().is_some()
                        && job.next_run
                            == codeterm_blueprint_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS))
                    && !self.wake_pending.contains(&job.id))
                    || event_trigger
                    || (input_waiting && !self.wake_pending.contains(&job.id))
                    || job.machine.paused
                    || active
                    || waiting_for_run
                {
                    None
                } else if job.machine.held.is_some() {
                    super::escalation::expiry_at(job)
                } else if self.wake_pending.contains(&job.id) {
                    Some(now)
                } else {
                    Some(job.next_run)
                };
                next.into_iter().chain(job.expires_at)
            })
            .min()
            .map(|at| at.saturating_sub(now))
            .unwrap_or(60_000)
            .min(60_000)
    }

    pub fn event_wait_only(&self) -> bool {
        let live = self
            .jobs
            .iter()
            .filter(|job| !job.machine.stopped)
            .collect::<Vec<_>>();
        !live.is_empty()
            && live.iter().all(|job| {
                job.expires_at.is_none()
                    && ((job.terminal().is_some() || job.input().is_some())
                        || job.pipeline().is_some_and(|pipeline| {
                            matches!(
                                &pipeline.trigger,
                                codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
                                    | codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
                            )
                        }))
            })
    }
}

impl Sentinel {
    pub fn probe(&self) -> &String {
        self.source.probe()
    }
    pub fn observe(&self) -> Option<&TabBinding> {
        self.source.observe()
    }
    pub fn input(&self) -> Option<&InputSpec> {
        self.source.input()
    }
    pub fn terminal(&self) -> Option<&codeterm_blueprint_types::blueprint_terminal::Config> {
        self.source.terminal()
    }
    pub fn pipeline(&self) -> Option<&codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec> {
        self.source.pipeline()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_probe(&mut self, probe: String) {
        self.source = super::stage_source::StageSource::Probe(probe);
    }
    pub fn set_input(&mut self, input: Option<InputSpec>) {
        if let Some(input) = input {
            self.source = super::stage_source::StageSource::Input(input);
        }
    }
    pub fn set_pipeline(
        &mut self,
        pipeline: Option<codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec>,
    ) {
        if let Some(pipeline) = pipeline {
            self.source = super::stage_source::StageSource::Pipeline(pipeline);
        }
    }
    pub fn set_observe(&mut self, binding: Option<TabBinding>) {
        if let Some(binding) = binding {
            self.source = match self.terminal().cloned() {
                Some(config) => super::stage_source::StageSource::Terminal { binding, config },
                None => super::stage_source::StageSource::Observe(binding),
            };
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_terminal(
        &mut self,
        config: Option<codeterm_blueprint_types::blueprint_terminal::Config>,
    ) {
        if let Some(binding) = self.observe().cloned() {
            self.source = match config {
                Some(config) => super::stage_source::StageSource::Terminal { binding, config },
                None => super::stage_source::StageSource::Observe(binding),
            };
        }
    }

    pub fn is_scenario_root(&self) -> bool {
        self.scenario_id.as_deref() == Some(self.id.as_str())
    }

    pub fn same_owner(&self, other: &Self) -> bool {
        self.creator_tab_id == other.creator_tab_id
            && self.team_id == other.team_id
            && self.workspace_id == other.workspace_id
    }

    /// The first stored graph capability this build cannot execute; such a stage is never
    /// created and never probed.
    pub fn pending_capability(&self) -> Option<&'static str> {
        let actions = || {
            self.routes
                .values()
                .flatten()
                .chain(&self.on_other)
                .map(|handler| &handler.action)
        };
        // Answering a prompt needs an observed agent tab; a shell probe has no prompt to answer.
        if self.observe().is_none()
            && actions().any(|action| matches!(action, Route::RespondPrompt(_)))
        {
            Some("respond_prompt")
        } else {
            None
        }
    }
}

pub fn scope_revisions(jobs: &[Sentinel]) -> BTreeMap<ScopeKey, String> {
    jobs.iter()
        .map(|j| (j.workspace_id.clone(), j.team_id.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|scope| {
            let revision = super::identity::scope_revision(jobs, &scope.0, scope.1.as_deref());
            (scope, revision)
        })
        .collect()
}

fn apply_finish(current: &mut Sentinel, job: &Sentinel, transition: Option<Transition>) {
    current.machine.last_hash = job.machine.last_hash.clone();
    current.machine.last_bucket = job.machine.last_bucket.clone();
    current.machine.stopped = job.machine.stopped;
    current.machine.held = job.machine.held.clone();
    current.last_reason = job.last_reason.clone();
    current.progress = job.progress.clone();
    current.run_transitions = job.run_transitions;
    current.transitions.extend(transition);
}

mod due;
pub use due::{action_limit, transition_budget_spent};
use due::{plan_due, scheduled_stop_notices};

mod validate;
pub use validate::duration_ms;
pub use validate::validate;
