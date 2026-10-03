use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde::{Deserialize, Serialize};

use super::handlers::{self, Definition, Handler, Transition};
use super::machine::{self, Machine, Route};
use crate::cron::clock::SharedClock;
use crate::persisted_state::StateFile;

const MAX_SENTINELS: usize = 256;
const MAX_ACTIVE: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub name: Option<String>,
    pub probe: String,
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
    pub cancelled: Arc<AtomicBool>,
}

pub struct Store {
    pub(super) jobs: Vec<Sentinel>,
    pub(super) active: HashMap<String, Arc<AtomicBool>>,
    file: StateFile,
    pub clock: SharedClock,
    unavailable: Option<LoadFailure>,
    committed_revisions: BTreeMap<(String, Option<String>), String>,
    changes: BTreeMap<(String, Option<String>), String>,
}

#[derive(Clone, Copy)]
enum LoadFailure {
    UnsupportedFormat,
    LoadFailed,
    PersistenceFailed,
}

impl LoadFailure {
    fn from_raw(raw: &str) -> Self {
        let unsupported = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|jobs| {
                jobs.iter().any(|job| {
                    job["routes"]
                        .as_object()
                        .is_some_and(|routes| routes.values().any(serde_json::Value::is_object))
                })
            });
        if unsupported {
            Self::UnsupportedFormat
        } else {
            Self::LoadFailed
        }
    }

    fn message(self) -> String {
        let code = match self {
            Self::UnsupportedFormat => "sentinel_store_unsupported_format",
            Self::LoadFailed => "sentinel_store_load_failed",
            Self::PersistenceFailed => "sentinel_store_persistence_failed",
        };
        format!("{code}: original definitions remain in place; preserve/export them and recreate through supported authoring only after operator recovery")
    }
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

fn decode(raw: &str) -> Result<Vec<Sentinel>, String> {
    let jobs: Vec<Sentinel> = serde_json::from_str(raw).map_err(|e| e.to_string())?;
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
                .routes
                .get(&transition.bucket)
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
                Route::Notify | Route::Stop => None,
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
        if job.probe.trim().is_empty()
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
    pub fn new(path: PathBuf, clock: SharedClock) -> Self {
        let file = StateFile::new(path);
        let (jobs, unavailable) = match std::fs::read_to_string(file.path()) {
            Ok(raw) => match decode(&raw) {
                Ok(jobs) => {
                    crate::persisted_state::release_hold(file.path());
                    (jobs, None)
                }
                Err(_) => {
                    file.hold_for_repair();
                    (Vec::new(), Some(LoadFailure::from_raw(&raw)))
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && file.held().is_none() => {
                (Vec::new(), None)
            }
            Err(_) => {
                file.hold_for_repair();
                (Vec::new(), Some(LoadFailure::LoadFailed))
            }
        };
        let committed_revisions = scope_revisions(&jobs);
        Self {
            jobs,
            active: HashMap::new(),
            file,
            clock,
            unavailable,
            committed_revisions,
            changes: BTreeMap::new(),
        }
    }

    pub(super) fn save(&mut self) -> Result<(), String> {
        self.ensure_available()?;
        self.check_existing_file()?;
        let raw = serde_json::to_string_pretty(&self.jobs).map_err(|e| e.to_string())?;
        if let Err(error) = self.file.save(&raw) {
            self.unavailable = Some(LoadFailure::PersistenceFailed);
            self.file.hold_for_repair();
            for cancelled in self.active.values() {
                cancelled.store(true, Ordering::SeqCst);
            }
            return Err(format!(
                "{}; write failed: {error}",
                LoadFailure::PersistenceFailed.message()
            ));
        }
        let current = scope_revisions(&self.jobs);
        let scopes: std::collections::BTreeSet<_> = current
            .keys()
            .chain(self.committed_revisions.keys())
            .cloned()
            .collect();
        for scope in scopes {
            if current.get(&scope) != self.committed_revisions.get(&scope) {
                let revision =
                    super::graph::scope_revision(&self.jobs, &scope.0, scope.1.as_deref());
                self.changes.insert(scope, revision);
            }
        }
        self.committed_revisions = current;
        Ok(())
    }

    fn check_existing_file(&mut self) -> Result<(), String> {
        let failure = match std::fs::read_to_string(self.file.path()) {
            Ok(raw) => decode(&raw).err().map(|_| LoadFailure::from_raw(&raw)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => Some(LoadFailure::LoadFailed),
        };
        if let Some(failure) = failure {
            self.unavailable = Some(failure);
            self.file.hold_for_repair();
            for cancelled in self.active.values() {
                cancelled.store(true, Ordering::SeqCst);
            }
            return Err(failure.message());
        }
        Ok(())
    }

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
        if self.file.held().is_some() {
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
        let interval_ms = duration_ms(&params.every)?;
        machine::validate(&params.buckets, &params.matches, connected, interval_ms)?;
        if params.probe.trim().is_empty()
            || params.probe.len() > 8192
            || params.question.trim().is_empty()
            || params.question.len() > 4096
        {
            return Err(
                "--probe and --question must be nonempty and bounded to 8192/4096 bytes".into(),
            );
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
        let routes = handlers::definitions(&params.buckets, params.routes, params.handlers)?;
        let composition = params.paused
            || declared
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
            probe: params.probe,
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
        };
        Ok(job)
    }

    pub fn insert(&mut self, job: Sentinel) -> Result<Sentinel, String> {
        self.ensure_available()?;
        if self.jobs.len() >= MAX_SENTINELS
            || (!job.machine.paused && self.active_count() >= MAX_ACTIVE)
        {
            return Err("sentinel capacity reached".into());
        }
        if self.get(&job.id).is_some() {
            return Err("duplicate sentinel ID".into());
        }
        self.validate_targets(&job)?;
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
                self.validate_target(&job, &id)?;
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
            return Err(error);
        }
        Ok(job)
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

    pub fn stop(&mut self, id: &str, reason: &str) -> Result<Sentinel, String> {
        self.ensure_available()?;
        let job = self
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("sentinel not found: {id}"))?;
        job.machine.stopped = true;
        job.revision += 1;
        job.last_reason = Some(reason.into());
        if let Some(cancelled) = self.active.remove(id) {
            cancelled.store(true, Ordering::SeqCst);
        }
        let result = job.clone();
        self.save()?;
        Ok(result)
    }

    pub fn remove_team(&mut self, team: &str) -> Result<(), String> {
        self.ensure_available()?;
        for job in self
            .jobs
            .iter()
            .filter(|job| job.team_id.as_deref() == Some(team))
        {
            if let Some(cancelled) = self.active.remove(&job.id) {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
        self.jobs.retain(|job| job.team_id.as_deref() != Some(team));
        self.save()
    }

    pub fn due(&mut self) -> Result<Vec<Lease>, String> {
        self.ensure_available()?;
        let now = self.clock.now_ms();
        let mut due = Vec::new();
        let previous = self.jobs.clone();
        let mut changed = false;
        for job in &mut self.jobs {
            if job.machine.stopped {
                continue;
            }
            if job.expires_at.is_some_and(|at| now >= at) {
                job.machine.stopped = true;
                job.revision += 1;
                job.last_reason = Some("timeout".into());
                if let Some(cancelled) = self.active.remove(&job.id) {
                    cancelled.store(true, Ordering::SeqCst);
                }
                changed = true;
                continue;
            }
            if !self.active.contains_key(&job.id)
                && (job.transitions.len() >= usize::from(job.max_transitions)
                    || job
                        .transitions
                        .iter()
                        .flat_map(|t| &t.handlers)
                        .filter(|r| r.claim_id.is_some())
                        .count()
                        >= usize::from(job.max_actions))
            {
                job.machine.stopped = true;
                job.revision += 1;
                job.last_reason = Some(
                    if job.transitions.len() >= usize::from(job.max_transitions) {
                        "transition_budget_exhausted"
                    } else {
                        "action_budget_exhausted"
                    }
                    .into(),
                );
                changed = true;
                continue;
            }
            if job.machine.paused {
                continue;
            }
            if job.next_run <= now && !self.active.contains_key(&job.id) {
                let missed = (now - job.next_run) / job.interval_ms;
                job.next_run = job
                    .next_run
                    .saturating_add((missed + 1).saturating_mul(job.interval_ms));
                let cancelled = Arc::new(AtomicBool::new(false));
                self.active.insert(job.id.clone(), cancelled.clone());
                due.push(Lease {
                    job: job.clone(),
                    cancelled,
                });
                changed = true;
            }
        }
        if changed {
            if let Err(error) = self.save() {
                for lease in &due {
                    lease.cancelled.store(true, Ordering::SeqCst);
                    self.active.remove(&lease.job.id);
                }
                self.jobs = previous;
                return Err(error);
            }
        }
        Ok(due)
    }

    pub fn finish(&mut self, job: &Sentinel) -> Result<bool, String> {
        self.ensure_available()?;
        let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) else {
            return Ok(false);
        };
        if !current.same_owner(job)
            || current.revision != job.revision
            || current.machine.stopped
            || current
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
        {
            return Ok(false);
        }
        let mut next = job.clone();
        if next.transitions.len() < current.transitions.len() {
            next.transitions = current.transitions.clone();
        }
        *current = next;
        if let Err(error) = self.save() {
            if let Some(current) = self.jobs.iter_mut().find(|current| current.id == job.id) {
                current.machine.stopped = true;
                current.last_reason = Some("persistence_failed".into());
            }
            return Err(error);
        }
        Ok(true)
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

    pub fn activate(
        &mut self,
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
        let previous = self.jobs.clone();
        let target = self
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or("sentinel target disappeared")?;
        target.machine.paused = false;
        target.revision += 1;
        if let Err(error) = self.save() {
            self.jobs = previous;
            return Err(error);
        }
        Ok(true)
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

    pub fn next_deadline(&self) -> u64 {
        let now = self.clock.now_ms();
        self.jobs
            .iter()
            .filter(|job| !job.machine.stopped)
            .flat_map(|job| {
                let next = if job.machine.paused || self.active.contains_key(&job.id) {
                    None
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
}

fn scope_revisions(jobs: &[Sentinel]) -> BTreeMap<(String, Option<String>), String> {
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
