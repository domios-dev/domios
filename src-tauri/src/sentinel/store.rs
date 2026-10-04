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
use crate::cron::clock::SharedClock;

const MAX_SENTINELS: usize = 256;
pub(super) const MAX_ACTIVE: usize = 64;

pub(super) type ScopeKey = (String, Option<String>);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub name: Option<String>,
    #[serde(default)]
    pub probe: Option<String>,
    #[serde(default)]
    pub observe: Option<TabBinding>,
    pub question: String,
    pub buckets: Vec<String>,
    #[serde(default = "default_every")]
    pub every: String,
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
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
#[serde(deny_unknown_fields)]
pub struct Sentinel {
    pub id: String,
    pub team_id: Option<String>,
    pub creator_tab_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub probe: String,
    pub question: String,
    pub buckets: Vec<String>,
    pub interval_ms: u64,
    pub routes: BTreeMap<String, Vec<Handler>>,
    pub matches: BTreeMap<String, String>,
    pub until: Option<String>,
    pub expires_at: Option<u64>,
    pub next_run: u64,
    pub machine: Machine,
    pub last_reason: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe: Option<TabBinding>,
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
    pub(super) cancelled: Arc<Cancellation>,
}

#[cfg(test)]
impl Lease {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub struct Store {
    pub(super) jobs: Vec<Sentinel>,
    pub(super) active: HashMap<String, Arc<Cancellation>>,
    pub(super) wake_pending: BTreeSet<String>,
    pub(super) backend: Backend,
    pub clock: SharedClock,
    pub(super) unavailable: Option<LoadFailure>,
    pub(super) admission_closed: bool,
    pub(super) committed_revisions: BTreeMap<ScopeKey, String>,
    pub(super) changes: BTreeMap<ScopeKey, String>,
}

/// Operational scheduling changes staged by `due`; never authored definition versions.
pub(super) enum ScheduleChange {
    Advance { id: String, next_run: u64 },
    Stop { id: String, reason: &'static str },
}

pub fn duration_ms(value: &str) -> Result<u64, String> {
    if value.len() > 32
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 's' | 'm' | 'h' | 'd'))
    {
        return Err("duration: use seconds, minutes, hours or days, e.g. 20s or 1h30m".into());
    }
    if value.chars().any(|c| c.is_ascii_alphabetic())
        && value.ends_with(|c: char| c.is_ascii_digit())
    {
        return Err("duration: every component needs a unit".into());
    }
    let mut number = String::new();
    let mut bound = 0u64;
    for c in value.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().map_err(|_| "duration: invalid number")?;
        number.clear();
        let factor = match c {
            's' => 1000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => unreachable!(),
        };
        bound = bound
            .checked_add(n.checked_mul(factor).ok_or("duration: overflow")?)
            .ok_or("duration: overflow")?;
    }
    if !number.is_empty() {
        bound = bound
            .checked_add(
                number
                    .parse::<u64>()
                    .map_err(|_| "duration: invalid number")?
                    .checked_mul(60_000)
                    .ok_or("duration: overflow")?,
            )
            .ok_or("duration: overflow")?;
    }
    if bound == 0 || bound > 366 * 86_400_000 {
        return Err("duration: must be positive and at most 366 days".into());
    }
    crate::cron::store::parse_oneshot_schedule_at(&format!("in {value}"), 0)
}

/// Every stored-inventory invariant, shared by the legacy file and typed Storage loads.
pub(super) fn validate(jobs: Vec<Sentinel>) -> Result<Vec<Sentinel>, String> {
    if jobs.len() > MAX_SENTINELS
        || jobs
            .iter()
            .filter(|job| !job.machine.stopped && !job.machine.paused)
            .count()
            > MAX_ACTIVE
    {
        return Err("sentinel store exceeds capacity".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for job in &jobs {
        machine::validate(&job.buckets, &job.matches, true, job.interval_ms)?;
        handlers::validate(&job.buckets, &job.routes)?;
        source::validate(job)?;
        escalation::validate(job)?;
        amend::validate(job)?;
        if job.max_transitions == 0
            || job.max_transitions > 256
            || job.max_actions == 0
            || job.max_actions > 1024
            || job.transitions.len() > usize::from(job.max_transitions)
            || job
                .transitions
                .iter()
                .flat_map(|t| &t.handlers)
                .filter(|r| r.claim_id.is_some())
                .count()
                > usize::from(job.max_actions)
        {
            return Err("invalid sentinel budgets or retention".into());
        }
        let mut transitions = std::collections::BTreeSet::new();
        for transition in &job.transitions {
            let declared = job
                .declared_handlers(&transition.bucket)
                .ok_or("stored transition has an undeclared bucket")?;
            if transition.id.is_empty()
                || transition.id.len() > 128
                || !transitions.insert(&transition.id)
                || transition.handlers.len() > declared.len()
            {
                return Err("invalid stored transition identity or handler retention".into());
            }
            let mut results = std::collections::BTreeSet::new();
            for result in &transition.handlers {
                if let Some(command) = &result.command_result {
                    command.validate()?;
                    let handler = declared
                        .iter()
                        .find(|handler| handler.id == result.handler_id)
                        .ok_or("command result has no declared handler")?;
                    let expected = match command.status {
                        super::command::Status::Success => handlers::Status::Completed,
                        super::command::Status::Unconfirmed => handlers::Status::Unconfirmed,
                        _ => handlers::Status::Failed,
                    };
                    if !matches!(handler.action, Route::Command(_))
                        || result.status != expected
                        || result.execution_id.is_none()
                    {
                        return Err("invalid stored command handler result identity".into());
                    }
                }
                if !results.insert(&result.handler_id)
                    || !declared
                        .iter()
                        .any(|handler| handler.id == result.handler_id)
                    || (result.claim_id.is_none() != result.accepted_at.is_none())
                    || (result.status == handlers::Status::Skipped
                        && (result.claim_id.is_some() || result.execution_id.is_some()))
                    || (result.status != handlers::Status::Skipped && result.claim_id.is_none())
                {
                    return Err("invalid stored handler claim identity".into());
                }
            }
        }
        for handler in job.routes.values().flatten() {
            let expected_kind_id = match &handler.action {
                Route::Sentinel(id) => Some(("sentinel", id)),
                Route::Send(id) => Some(("worker", id)),
                Route::Cron(id) => Some(("cron", id)),
                Route::Notify
                | Route::Stop
                | Route::Command(_)
                | Route::RespondPrompt(_)
                | Route::Escalate(_) => None,
            };
            if let Some((kind, id)) = expected_kind_id {
                Route::parse(&format!(
                    "{}:{id}",
                    if kind == "worker" { "send" } else { kind }
                ))?;
            }
            if let Some(target) = &handler.target {
                if expected_kind_id != Some((target.kind.as_str(), &target.id))
                    || target.workspace_id != job.workspace_id
                    || target.team_id != job.team_id
                    || (target.kind == "sentinel"
                        && target.owner_tab_id.as_deref() != Some(&job.creator_tab_id))
                {
                    return Err("invalid stored exact handler target scope".into());
                }
            }
        }
        if !ids.insert(&job.id)
            || job.creator_tab_id.is_empty()
            || job.workspace_id.is_empty()
            || job.cwd.is_empty()
        {
            return Err("invalid sentinel identity or scope".into());
        }
        if (job.observe.is_none() && job.probe.trim().is_empty())
            || job.probe.len() > 8192
            || job.question.trim().is_empty()
            || job.question.len() > 4096
        {
            return Err("invalid sentinel probe or question".into());
        }
        if job
            .routes
            .keys()
            .any(|bucket| !job.buckets.contains(bucket))
            || job
                .until
                .as_ref()
                .is_some_and(|bucket| !job.buckets.contains(bucket))
        {
            return Err("invalid sentinel route bucket".into());
        }
    }
    for job in &jobs {
        if let Some(scenario) = &job.scenario_id {
            if !jobs.iter().any(|root| {
                root.id == *scenario
                    && root.scenario_id.as_deref() == Some(root.id.as_str())
                    && root.same_owner(job)
            }) {
                return Err("invalid stored scenario ownership association".into());
            }
        }
    }
    Ok(jobs)
}

impl Store {
    pub fn committed_revision(&self, workspace: &str, team: Option<&str>) -> Option<&str> {
        self.committed_revisions
            .get(&(workspace.into(), team.map(str::to_owned)))
            .map(String::as_str)
    }

    pub fn take_changes(&mut self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.changes).into_iter().map(|((workspace_id,team_id),revision)| serde_json::json!({"workspace_id":workspace_id,"team_id":team_id,"revision":revision})).collect()
    }

    pub fn ensure_available(&self) -> Result<(), String> {
        if let Some(failure) = self.unavailable {
            return Err(failure.message());
        }
        if self.backend.held() {
            return Err("sentinel_store_load_failed: persistence is held for repair".into());
        }
        Ok(())
    }

    pub fn list(&self) -> &[Sentinel] {
        &self.jobs
    }
    pub fn get(&self, id: &str) -> Option<&Sentinel> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn prepare(
        &self,
        params: Create,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        if self.admission_closed {
            return Err("sentinel_shutdown_admission_closed".into());
        }
        self.ensure_available()?;
        let Owner {
            creator_tab_id: creator,
            team_id: team,
            workspace_id: workspace,
            cwd,
        } = owner;
        if params.name.as_ref().is_some_and(|name| {
            name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control)
        }) {
            return Err("--name: provide a nonempty name up to 256 bytes".into());
        }
        let (probe, observe) = match (params.probe, params.observe) {
            (Some(probe), None) if !probe.trim().is_empty() && probe.len() <= 8192 => (probe, None),
            (Some(_), None) => {
                return Err("--probe must be nonempty and bounded to 8192 bytes".into());
            }
            (None, Some(binding @ TabBinding::Tab(_))) if source::bounded_id(binding.id()) => {
                (String::new(), Some(binding))
            }
            (Some(_), Some(_)) => return Err("--probe conflicts with --observe".into()),
            (None, Some(_)) => return Err("--observe: use tab:<id>".into()),
            _ => return Err("provide exactly one of --probe or --observe tab:<id>".into()),
        };
        let interval_ms = duration_ms(&params.every)?;
        machine::validate(&params.buckets, &params.matches, connected, interval_ms)?;
        if params.question.trim().is_empty() || params.question.len() > 4096 {
            return Err("--question must be nonempty and bounded to 4096 bytes".into());
        }
        if self.jobs.len() >= MAX_SENTINELS || (!params.paused && self.active_count() >= MAX_ACTIVE)
        {
            return Err("sentinel capacity reached (64 active, 256 total); stop active sentinels or close their Team".into());
        }
        if params
            .until
            .as_ref()
            .is_some_and(|bucket| !params.buckets.contains(bucket))
        {
            return Err("--until: name a declared bucket".into());
        }
        let declared = params.handlers.is_some();
        let (routes, on_other) =
            handlers::definitions_with_other(&params.buckets, params.routes, params.handlers)?;
        let composition = params.paused
            || declared
            || !on_other.is_empty()
            || routes.values().any(|hs| {
                hs.len() > 1 || hs.iter().any(|h| matches!(h.action, Route::Sentinel(_)))
            });
        if composition && params.timeout.is_none() {
            return Err("composition sentinels require an explicit --timeout".into());
        }
        let max_transitions = params
            .max_transitions
            .unwrap_or(handlers::DEFAULT_TRANSITIONS);
        let max_actions = params.max_actions.unwrap_or(handlers::DEFAULT_ACTIONS);
        if max_transitions == 0 || max_transitions > 256 || max_actions == 0 || max_actions > 1024 {
            return Err(
                "sentinel budgets must be positive, at most 256 transitions and 1024 actions"
                    .into(),
            );
        }
        let now = self.clock.now_ms();
        let expires_at = params
            .timeout
            .as_deref()
            .map(duration_ms)
            .transpose()?
            .map(|duration| now.saturating_add(duration));
        let id = format!(
            "sentinel_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let job = Sentinel {
            id: id.clone(),
            team_id: team,
            creator_tab_id: creator,
            workspace_id: workspace,
            cwd,
            probe,
            question: params.question,
            buckets: params.buckets,
            interval_ms,
            routes,
            matches: params.matches,
            until: params.until,
            expires_at,
            next_run: now,
            machine: Machine {
                paused: params.paused,
                ..Machine::default()
            },
            last_reason: None,
            revision: 0,
            agent_authored: false,
            name: params.name,
            scenario_id: if params.paused { None } else { Some(id) },
            max_transitions,
            max_actions,
            transitions: Vec::new(),
            observe,
            run: None,
            on_other,
            escalation_timeout_ms: params.escalation_timeout_ms,
            definition_version: amend::initial_version(),
            amendments: Vec::new(),
        };
        source::validate(&job)?;
        escalation::validate(&job)?;
        Ok(job)
    }

    pub(crate) fn stage_insert(&mut self, job: Sentinel) -> Staged<Sentinel> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        if let Some(capability) = job.pending_capability() {
            return Staged::ready(Err(format!(
                "sentinel_capability_unavailable: {capability} is not executable yet"
            )));
        }
        if self.jobs.len() >= MAX_SENTINELS
            || (!job.machine.paused && self.active_count() >= MAX_ACTIVE)
        {
            return Staged::ready(Err("sentinel capacity reached".into()));
        }
        if self.get(&job.id).is_some() {
            return Staged::ready(Err("duplicate sentinel ID".into()));
        }
        let association_ids = match self.association_ids(&job) {
            Ok(ids) => ids,
            Err(error) => return Staged::ready(Err(error)),
        };
        if self.backend.json().is_none() {
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
        if let Err(error) = self.save() {
            self.jobs = previous;
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(job))
    }

    /// Validates exact targets and collects unassociated stages joining the new scenario.
    pub(super) fn association_ids(&self, job: &Sentinel) -> Result<Vec<String>, String> {
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

    #[cfg(test)]
    pub fn insert(&mut self, job: Sentinel) -> Result<Sentinel, String> {
        self.locked(|store| store.stage_insert(job))
    }

    #[cfg(test)]
    pub fn create(
        &mut self,
        params: Create,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let job = self.prepare(params, owner, connected)?;
        self.insert(job)
    }

    pub(super) fn close_admission(&mut self) {
        self.admission_closed = true;
        for cancellation in self.active.values() {
            cancellation.cancel();
        }
    }

    pub(crate) fn stage_stop(&mut self, id: &str, reason: &str) -> Staged<Sentinel> {
        if self.get(id).is_none() {
            return Staged::ready(Err(format!("sentinel not found: {id}")));
        }
        if let Some(cancelled) = self.active.remove(id) {
            cancelled.store(true, Ordering::SeqCst);
        }
        if self.backend.json().is_none() {
            return self.sqlite_stop(id, reason);
        }
        let Some(stopped) = self.apply_stop(id, reason) else {
            return Staged::ready(Err(format!("sentinel not found: {id}")));
        };
        Staged::ready(self.save().map(|()| stopped))
    }

    /// The in-memory stop; callers cancel the live lease themselves.
    pub(super) fn apply_stop(&mut self, id: &str, reason: &str) -> Option<Sentinel> {
        let job = self.jobs.iter_mut().find(|job| job.id == id)?;
        job.machine.stopped = true;
        job.revision += 1;
        job.last_reason = Some(reason.into());
        self.wake_pending.remove(id);
        Some(job.clone())
    }

    #[cfg(test)]
    pub fn stop(&mut self, id: &str, reason: &str) -> Result<Sentinel, String> {
        self.locked(|store| store.stage_stop(id, reason))
    }

    pub(super) fn stage_remove_team(&mut self, team: &str) -> Staged<()> {
        for job in self
            .jobs
            .iter()
            .filter(|job| job.team_id.as_deref() == Some(team))
        {
            if let Some(cancelled) = self.active.remove(&job.id) {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
        if self.backend.json().is_none() {
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
        Staged::ready(self.save())
    }

    #[cfg(test)]
    pub fn remove_team(&mut self, team: &str) -> Result<(), String> {
        self.locked(|store| store.stage_remove_team(team))
    }

    /// The typed backend plans on a copy, so the cache changes only on publication.
    pub(super) fn stage_due(&mut self) -> Staged<Vec<Lease>> {
        if self.admission_closed {
            return Staged::ready(Err("sentinel_shutdown_admission_closed".into()));
        }
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let now = self.clock.now_ms();
        if self.backend.json().is_none() {
            let mut planned = self.jobs.clone();
            let (due, changes, consumed_wakes) =
                plan_due(&mut planned, &mut self.active, &mut self.wake_pending, now);
            return self.sqlite_due(due, changes, consumed_wakes);
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
            if let Err(error) = self.save() {
                for lease in &due {
                    lease.cancelled.store(true, Ordering::SeqCst);
                    self.active.remove(&lease.job.id);
                }
                self.jobs = previous;
                self.wake_pending = previous_wakes;
                return Staged::ready(Err(error));
            }
        }
        Staged::ready(Ok(due))
    }

    #[cfg(test)]
    pub fn due(&mut self) -> Result<Vec<Lease>, String> {
        self.locked(Store::stage_due)
    }

    pub(super) fn stage_finish(&mut self, job: &Sentinel) -> Staged<bool> {
        self.stage_finish_with(job, None)
    }

    pub(crate) fn stage_resolve_escalation(
        &mut self,
        id: &str,
        transition_id: &str,
    ) -> Staged<bool> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(current) = self.get(id).cloned() else {
            return Staged::ready(Err(format!("sentinel not found: {id}")));
        };
        if current.machine.held.as_deref() != Some(transition_id) {
            return Staged::ready(Err(
                "sentinel escalation is not open for that transition".into()
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

    pub(crate) fn wake_stage(&mut self, id: &str) -> bool {
        let Some(job) = self.get(id) else {
            return false;
        };
        if job.machine.stopped || job.machine.paused || job.machine.held.is_some() {
            return false;
        }
        self.wake_pending.insert(id.into())
    }

    pub(super) fn stage_expire_escalation(&mut self) -> Option<(Sentinel, Staged<bool>)> {
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
    pub(super) fn stage_finish_with(
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
        if self.backend.json().is_none() {
            return self.sqlite_finish(job, transition);
        }
        if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
            apply_finish(current, job, transition);
        }
        if let Err(error) = self.save() {
            if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
                current.machine.stopped = true;
                current.last_reason = Some("persistence_failed".into());
            }
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(true))
    }

    /// A failed commit keeps the outcome in memory but stops the stage, as the file did.
    pub(super) fn keep_finish_intent(&mut self, job: &Sentinel, transition: Option<Transition>) {
        if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
            apply_finish(current, job, transition);
            current.machine.stopped = true;
            current.last_reason = Some("persistence_failed".into());
        }
    }

    #[cfg(test)]
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
            return Err("sentinel cannot activate itself".into());
        }
        let target = self.get(id).ok_or("sentinel target not found")?;
        if !source.same_owner(target) {
            return Err("sentinel target requires the same creator, Team and workspace".into());
        }
        if target.machine.stopped
            || target
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
        {
            return Err("sentinel target is stopped or expired".into());
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

    pub(super) fn stage_activate(
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
        if self.backend.json().is_none() {
            return self.sqlite_activate(source, transition, id);
        }
        let previous = self.jobs.clone();
        let Some(target) = self.jobs.iter_mut().find(|job| job.id == id) else {
            return Staged::ready(Err("sentinel target disappeared".into()));
        };
        target.machine.paused = false;
        target.revision += 1;
        if let Err(error) = self.save() {
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
            return Err("sentinel activation source fenced".into());
        }
        let current = self
            .get(&source.id)
            .ok_or("sentinel activation source disappeared")?;
        let transition = current
            .transitions
            .iter()
            .find(|record| record.id == transition)
            .ok_or("sentinel activation transition disappeared")?;
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
            return Err("sentinel activation requires its declared durable handler claim".into());
        }
        self.validate_target(source, id)?;
        if !self
            .get(id)
            .ok_or("sentinel target disappeared")?
            .machine
            .paused
        {
            return Ok(false);
        }
        if self.active_count() >= MAX_ACTIVE {
            return Err("sentinel active capacity reached".into());
        }
        Ok(true)
    }

    #[cfg(test)]
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
                    && !job.machine.paused
                    && !job.expires_at.is_some_and(|at| now >= at)
                    && matches!(job.observe.as_ref(), Some(TabBinding::Tab(id)) if id == tab_id)
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
                let next = if job.machine.paused || active {
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
}

impl Sentinel {
    pub fn same_owner(&self, other: &Self) -> bool {
        self.creator_tab_id == other.creator_tab_id
            && self.team_id == other.team_id
            && self.workspace_id == other.workspace_id
    }

    /// The first stored graph capability this build cannot execute; such a stage is never
    /// created and never probed.
    pub(super) fn pending_capability(&self) -> Option<&'static str> {
        let actions = || {
            self.routes
                .values()
                .flatten()
                .chain(&self.on_other)
                .map(|handler| &handler.action)
        };
        if self
            .observe
            .as_ref()
            .is_some_and(|binding| !matches!(binding, TabBinding::Tab(_)))
            || self.run.is_some()
        {
            Some("tab observation")
        } else if actions().any(|action| matches!(action, Route::RespondPrompt(_))) {
            Some("respond_prompt")
        } else {
            None
        }
    }
}

pub(super) fn scope_revisions(jobs: &[Sentinel]) -> BTreeMap<ScopeKey, String> {
    jobs.iter()
        .map(|j| (j.workspace_id.clone(), j.team_id.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|scope| {
            let revision = super::graph::scope_revision(jobs, &scope.0, scope.1.as_deref());
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
    current.transitions.extend(transition);
}

/// Expiry and budget stops, then due leases; registers each lease's ephemeral cancellation.
fn plan_due(
    jobs: &mut [Sentinel],
    active: &mut HashMap<String, Arc<Cancellation>>,
    wake_pending: &mut BTreeSet<String>,
    now: u64,
) -> (Vec<Lease>, Vec<ScheduleChange>, Vec<String>) {
    let mut due = Vec::new();
    let mut changes = Vec::new();
    let mut consumed_wakes = Vec::new();
    for job in jobs.iter_mut() {
        if job.machine.stopped {
            continue;
        }
        if job.expires_at.is_some_and(|at| now >= at) {
            wake_pending.remove(&job.id);
            job.machine.stopped = true;
            job.revision += 1;
            job.last_reason = Some("timeout".into());
            if let Some(cancelled) = active.remove(&job.id) {
                cancelled.store(true, Ordering::SeqCst);
            }
            changes.push(ScheduleChange::Stop {
                id: job.id.clone(),
                reason: "timeout",
            });
            continue;
        }
        if job.machine.held.is_some() {
            continue;
        }
        let transitions_spent = job.transitions.len() >= usize::from(job.max_transitions);
        if !active.contains_key(&job.id)
            && (transitions_spent
                || job
                    .transitions
                    .iter()
                    .flat_map(|t| &t.handlers)
                    .filter(|r| r.claim_id.is_some())
                    .count()
                    >= usize::from(job.max_actions))
        {
            let reason = if transitions_spent {
                "transition_budget_exhausted"
            } else {
                "action_budget_exhausted"
            };
            job.machine.stopped = true;
            wake_pending.remove(&job.id);
            job.revision += 1;
            job.last_reason = Some(reason.into());
            changes.push(ScheduleChange::Stop {
                id: job.id.clone(),
                reason,
            });
            continue;
        }
        if job.machine.paused {
            continue;
        }
        if (job.next_run <= now || wake_pending.contains(&job.id)) && !active.contains_key(&job.id)
        {
            if job.next_run <= now {
                let missed = (now - job.next_run) / job.interval_ms;
                job.next_run = job
                    .next_run
                    .saturating_add((missed + 1).saturating_mul(job.interval_ms));
            } else {
                job.next_run = now.saturating_add(job.interval_ms);
            }
            if wake_pending.remove(&job.id) {
                consumed_wakes.push(job.id.clone());
            }
            let cancelled = Arc::new(Cancellation::new(false));
            active.insert(job.id.clone(), cancelled.clone());
            due.push(Lease {
                job: job.clone(),
                cancelled,
            });
            changes.push(ScheduleChange::Advance {
                id: job.id.clone(),
                next_run: job.next_run,
            });
        }
    }
    (due, changes, consumed_wakes)
}
