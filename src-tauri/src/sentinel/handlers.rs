use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::machine::{Route, SafeKind};
use super::storage::Staged;
use super::store::{Sentinel, Store};

pub const MAX_HANDLERS_PER_BUCKET: usize = 16;
pub const MAX_HANDLERS: usize = 64;
pub const DEFAULT_TRANSITIONS: u16 = 64;
pub const DEFAULT_ACTIONS: u16 = 256;
pub const ESCALATION_RATE_WINDOW_MS: u64 = 300_000;
pub const ESCALATION_RATE_LIMITED: &str = "sentinel_escalation_rate_limited";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handler {
    pub id: String,
    pub action: Route,
    #[serde(default)]
    pub target: Option<super::graph::Identity>,
}

impl Handler {
    pub fn notify(bucket: &str) -> Self {
        Self {
            id: format!("{bucket}_notify"),
            action: Route::Notify,
            target: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub parameters: Parameters,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub target: Option<String>,
    pub command: Option<String>,
    pub label: Option<String>,
    pub timeout_ms: Option<u64>,
}

impl Definition {
    fn resolve(self) -> Result<Handler, String> {
        let parameters = self.parameters;
        let has_command = parameters.command.is_some()
            || parameters.label.is_some()
            || parameters.timeout_ms.is_some();
        let action = if self.kind == "command" {
            if parameters.target.is_some() {
                return Err("command has no target parameter".into());
            }
            let command = super::command::Parameters {
                command: parameters.command.ok_or("command text is required")?,
                label: parameters.label.ok_or("command label is required")?,
                timeout_ms: parameters
                    .timeout_ms
                    .ok_or("command timeout_ms is required")?,
            };
            command.validate()?;
            Route::Command(command)
        } else {
            if has_command {
                return Err("command parameters require type=command".into());
            }
            match (self.kind.as_str(), parameters.target) {
                ("notify", None) => Route::Notify,
                ("stop", None) => Route::Stop,
                (kind @ ("sentinel" | "send" | "cron" | "respond_prompt"), Some(target)) => {
                    Route::parse(&format!("{kind}:{target}"))?
                }
                ("escalate", Some(target)) => Route::parse(&format!("escalate:{target}"))?,
                _ => return Err("handler type or parameters are invalid".into()),
            }
        };
        Ok(Handler {
            id: self.id,
            action,
            target: None,
        })
    }
}

pub fn definitions(
    buckets: &[String],
    routes: BTreeMap<String, String>,
    declared: Option<BTreeMap<String, Vec<Definition>>>,
) -> Result<BTreeMap<String, Vec<Handler>>, String> {
    if !routes.is_empty() && declared.is_some() {
        return Err("--route and --handlers-file cannot be combined".into());
    }
    let mut result = BTreeMap::new();
    if let Some(declared) = declared {
        for (bucket, definitions) in declared {
            let handlers = definitions
                .into_iter()
                .map(Definition::resolve)
                .collect::<Result<_, _>>()?;
            result.insert(bucket, handlers);
        }
    } else {
        for (index, (bucket, route)) in routes.into_iter().enumerate() {
            result.insert(
                bucket.clone(),
                vec![Handler {
                    id: format!("route_{}", index + 1),
                    action: Route::parse(&route)?,
                    target: None,
                }],
            );
        }
    }
    for (index, bucket) in buckets.iter().enumerate() {
        result.entry(bucket.clone()).or_insert_with(|| {
            vec![Handler {
                id: format!("default_{}", index + 1),
                action: Route::Notify,
                target: None,
            }]
        });
    }
    validate(buckets, &result)?;
    Ok(result)
}

pub fn definitions_with_other(
    buckets: &[String],
    routes: BTreeMap<String, String>,
    declared: Option<BTreeMap<String, Vec<Definition>>>,
) -> Result<(BTreeMap<String, Vec<Handler>>, Vec<Handler>), String> {
    let mut declared = declared;
    let on_other = declared
        .as_mut()
        .and_then(|handlers| handlers.remove(crate::storage::OTHER_BUCKET));
    let routes = definitions(buckets, routes, declared)?;
    let on_other = match on_other {
        Some(handlers) => definitions(
            &[crate::storage::OTHER_BUCKET.into()],
            BTreeMap::new(),
            Some(BTreeMap::from([(
                crate::storage::OTHER_BUCKET.into(),
                handlers,
            )])),
        )?
        .remove(crate::storage::OTHER_BUCKET)
        .unwrap_or_default(),
        None => Vec::new(),
    };
    Ok((routes, on_other))
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

pub fn validate(
    buckets: &[String],
    handlers: &BTreeMap<String, Vec<Handler>>,
) -> Result<(), String> {
    if handlers.len() != buckets.len() {
        return Err("each declared bucket requires a handler array".into());
    }
    let mut ids = BTreeSet::new();
    for (bucket, handlers) in handlers {
        if !buckets.contains(bucket) || handlers.len() > MAX_HANDLERS_PER_BUCKET {
            return Err(
                "handlers require declared buckets and at most 16 handlers per bucket".into(),
            );
        }
        for handler in handlers {
            if let Route::Command(parameters) = &handler.action {
                parameters.validate()?;
            }
            if let Route::RespondPrompt(SafeKind::Other(kind)) = &handler.action {
                if kind.trim().is_empty() || kind.len() > 256 || kind.chars().any(char::is_control)
                {
                    return Err("respond-prompt kind must be nonempty and bounded".into());
                }
            }

            if !valid_id(&handler.id) || !ids.insert(&handler.id) {
                return Err(
                    "handler IDs must be unique bounded letters, numbers, underscores or dashes"
                        .into(),
                );
            }
        }
    }
    if ids.len() > MAX_HANDLERS {
        return Err("at most 64 handlers per sentinel".into());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unconfirmed,
    Completed,
    Failed,
    Noop,
    Skipped,
}

pub struct Effect {
    pub status: Status,
    pub execution_id: Option<String>,
    pub command_result: Option<super::command::ResultMetadata>,
}

impl From<Status> for Effect {
    fn from(status: Status) -> Self {
        Self {
            status,
            execution_id: None,
            command_result: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRecord {
    pub handler_id: String,
    pub status: Status,
    pub claim_id: Option<String>,
    pub execution_id: Option<String>,
    pub accepted_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_result: Option<super::command::ResultMetadata>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub id: String,
    pub bucket: String,
    pub accepted_at: u64,
    pub terminal: bool,
    pub handlers: Vec<ResultRecord>,
}

pub fn default_transitions() -> u16 {
    DEFAULT_TRANSITIONS
}
pub fn default_actions() -> u16 {
    DEFAULT_ACTIONS
}

/// Admission decision of one handler claim, taken under the store lock.
enum Gate {
    Refuse,
    Exhausted,
    RateLimited,
    Claim,
}

fn apply_effect(record: &mut ResultRecord, effect: Effect) {
    record.execution_id = effect.execution_id.or_else(|| match effect.status {
        Status::Completed | Status::Noop => record.claim_id.clone(),
        _ => None,
    });
    record.reason = match effect.status {
        Status::Failed => Some("action_failed".into()),
        Status::Unconfirmed => Some("effect_unconfirmed".into()),
        _ => None,
    };
    record.command_result = effect.command_result;
    record.status = effect.status;
}

fn skipped(handler: &Handler) -> ResultRecord {
    ResultRecord {
        handler_id: handler.id.clone(),
        status: Status::Skipped,
        claim_id: None,
        execution_id: None,
        accepted_at: None,
        command_result: None,
        reason: Some("batch_fenced".into()),
    }
}

impl Store {
    pub(super) fn stage_accept_transition(
        &mut self,
        job: &Sentinel,
        bucket: &str,
        id: &str,
    ) -> Staged<bool> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let declared = job.buckets.iter().any(|declared| declared == bucket)
            || (bucket == crate::storage::OTHER_BUCKET && !job.on_other.is_empty());
        if id.is_empty() || id.len() > 128 || !declared {
            return Staged::ready(Err(
                "transition requires a stable identity and declared or reserved bucket".into(),
            ));
        }
        let now = self.clock.now_ms();
        let Some(current) = self.get(&job.id) else {
            return Staged::ready(Ok(false));
        };
        if current
            .transitions
            .iter()
            .any(|transition| transition.id == id)
        {
            return Staged::ready(Ok(false));
        }
        if !current.same_owner(job)
            || current.revision != job.revision
            || current.machine.stopped
            || current.machine.paused
            || current.expires_at.is_some_and(|at| now >= at)
        {
            return Staged::ready(Ok(false));
        }
        if super::store::transition_budget_spent(current) {
            return self
                .stage_stop(&job.id, "transition_budget_exhausted")
                .map(|_| false);
        }
        let transition = Transition {
            id: id.into(),
            bucket: bucket.into(),
            accepted_at: now,
            terminal: job.machine.stopped,
            handlers: Vec::new(),
        };
        let mut accepted = job.clone();
        if matches!(
            current.observe.as_ref(),
            Some(super::source::TabBinding::CronRun(_))
        ) && current.run.is_some()
        {
            accepted.run_transitions = current.run_transitions.saturating_add(1);
        }
        self.stage_finish_with(&accepted, Some(transition))
    }

    #[cfg(test)]
    pub fn accept_transition(
        &mut self,
        job: &Sentinel,
        bucket: &str,
        id: &str,
    ) -> Result<bool, String> {
        self.locked(|store| store.stage_accept_transition(job, bucket, id))
    }

    fn batch_live(&self, job: &Sentinel, transition_id: &str) -> bool {
        if self.admission_closed || self.ensure_available().is_err() {
            return false;
        }
        self.get(&job.id).is_some_and(|current| {
            current.same_owner(job)
                && current.revision == job.revision
                && current.machine.last_hash == job.machine.last_hash
                && current.machine.last_bucket == job.machine.last_bucket
                && current
                    .transitions
                    .last()
                    .is_some_and(|transition| transition.id == transition_id)
                && !current.machine.paused
                && current.expires_at.is_none_or(|at| self.clock.now_ms() < at)
                && current
                    .transitions
                    .iter()
                    .any(|t| t.id == transition_id && (!current.machine.stopped || t.terminal))
                && self
                    .active
                    .get(&job.id)
                    .is_none_or(|c| !c.load(std::sync::atomic::Ordering::SeqCst))
        })
    }

    fn claim_gate(
        &self,
        job: &Sentinel,
        transition_id: &str,
        handler: &Handler,
    ) -> Result<Gate, String> {
        if !self.batch_live(job, transition_id) {
            return Ok(Gate::Refuse);
        }
        let current = self.get(&job.id).ok_or("sentinel disappeared")?;
        let transition = current
            .transitions
            .iter()
            .find(|t| t.id == transition_id)
            .ok_or("transition missing")?;
        if transition
            .handlers
            .iter()
            .any(|r| r.handler_id == handler.id)
        {
            return Ok(Gate::Refuse);
        }
        if !current
            .declared_handlers(&transition.bucket)
            .is_some_and(|handlers| handlers.contains(handler))
        {
            return Err("handler is not declared for the accepted transition".into());
        }
        let now = self.clock.now_ms();
        if matches!(&handler.action, Route::Escalate(_))
            && current.transitions.iter().any(|transition| {
                transition.bucket == crate::storage::OTHER_BUCKET
                    && transition.handlers.iter().any(|result| {
                        result.handler_id == handler.id
                            && result.claim_id.is_some()
                            && result.accepted_at.is_some_and(|at| {
                                now.saturating_sub(at) < ESCALATION_RATE_WINDOW_MS
                            })
                    })
            })
        {
            return Ok(Gate::RateLimited);
        }
        let actions = current
            .transitions
            .iter()
            .flat_map(|t| &t.handlers)
            .filter(|r| r.claim_id.is_some())
            .count();
        if actions >= usize::from(current.max_actions) {
            return Ok(Gate::Exhausted);
        }
        Ok(Gate::Claim)
    }

    /// Durable claim-before-action: no effect may start until this settles with a claim.
    pub(super) fn stage_claim_handler(
        &mut self,
        job: &Sentinel,
        transition_id: &str,
        handler: &Handler,
    ) -> Staged<Option<ResultRecord>> {
        match self.claim_gate(job, transition_id, handler) {
            Ok(Gate::Claim) => {}
            Ok(Gate::Refuse) => return Staged::ready(Ok(None)),
            Ok(Gate::RateLimited) => return Staged::ready(Err(ESCALATION_RATE_LIMITED.into())),
            Ok(Gate::Exhausted) => {
                return self
                    .stage_stop(&job.id, "action_budget_exhausted")
                    .map(|_| None)
            }
            Err(error) => return Staged::ready(Err(error)),
        }
        let claim = ResultRecord {
            handler_id: handler.id.clone(),
            status: Status::Unconfirmed,
            claim_id: Some(uuid::Uuid::new_v4().to_string()),
            execution_id: None,
            accepted_at: Some(self.clock.now_ms()),
            command_result: None,
            reason: None,
        };
        if !self.backend.is_json() {
            return self.sqlite_claim(job, transition_id, claim);
        }
        let previous = self.jobs.clone();
        let Some(transition) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == job.id)
            .and_then(|j| j.transitions.iter_mut().find(|t| t.id == transition_id))
        else {
            return Staged::ready(Err("transition missing".into()));
        };
        transition.handlers.push(claim.clone());
        if let Err(error) = self.save_legacy() {
            self.jobs = previous;
            if let Some(cancelled) = self.active.get(&job.id) {
                cancelled.store(true, std::sync::atomic::Ordering::SeqCst)
            }
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(Some(claim)))
    }

    #[cfg(test)]
    pub fn claim_handler(
        &mut self,
        job: &Sentinel,
        transition_id: &str,
        handler: &Handler,
    ) -> Result<Option<ResultRecord>, String> {
        self.locked(|store| store.stage_claim_handler(job, transition_id, handler))
    }

    pub fn action_permitted(&self, job: &Sentinel, transition: &str) -> bool {
        self.batch_live(job, transition)
    }

    #[cfg(test)]
    pub fn finish_handler(
        &mut self,
        source: &str,
        transition_id: &str,
        claim: &ResultRecord,
        status: Status,
    ) -> Result<(), String> {
        self.finish_handler_result(source, transition_id, claim, status.into())
    }

    #[cfg(test)]
    pub fn finish_handler_result(
        &mut self,
        source: &str,
        transition_id: &str,
        claim: &ResultRecord,
        effect: Effect,
    ) -> Result<(), String> {
        self.locked(|store| store.stage_finish_handler_result(source, transition_id, claim, effect))
    }

    /// Records the effect of an existing claim; later stops never fence this record.
    pub(super) fn stage_finish_handler_result(
        &mut self,
        source: &str,
        transition_id: &str,
        claim: &ResultRecord,
        effect: Effect,
    ) -> Staged<()> {
        let typed = !self.backend.is_json();
        let previous = if typed { Vec::new() } else { self.jobs.clone() };
        let Some(record) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == source)
            .and_then(|j| j.transitions.iter_mut().find(|t| t.id == transition_id))
            .and_then(|t| {
                t.handlers
                    .iter_mut()
                    .find(|r| r.handler_id == claim.handler_id && r.claim_id == claim.claim_id)
            })
        else {
            return Staged::ready(Ok(()));
        };
        if typed {
            let mut updated = record.clone();
            apply_effect(&mut updated, effect);
            return self.sqlite_handler_result(source, transition_id, &updated);
        }
        apply_effect(record, effect);
        if let Err(error) = self.save_legacy() {
            self.jobs = previous;
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(()))
    }

    pub(super) fn stage_skip_remaining(
        &mut self,
        source: &str,
        transition_id: &str,
        handlers: &[Handler],
    ) -> Staged<()> {
        let typed = !self.backend.is_json();
        let previous = if typed { Vec::new() } else { self.jobs.clone() };
        let Some(transition) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == source)
            .and_then(|j| j.transitions.iter_mut().find(|t| t.id == transition_id))
        else {
            return Staged::ready(Ok(()));
        };
        let mut fenced: Vec<ResultRecord> = Vec::new();
        for handler in handlers {
            if !transition
                .handlers
                .iter()
                .chain(&fenced)
                .any(|r| r.handler_id == handler.id)
            {
                fenced.push(skipped(handler));
            }
        }
        if typed {
            return self.sqlite_skip(source, transition_id, &fenced);
        }
        transition.handlers.extend(fenced);
        if let Err(error) = self.save_legacy() {
            self.jobs = previous;
            return Staged::ready(Err(error));
        }
        Staged::ready(Ok(()))
    }
}
