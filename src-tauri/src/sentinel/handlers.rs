use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::machine::Route;
use super::store::{Sentinel, Store};

pub const MAX_HANDLERS_PER_BUCKET: usize = 16;
pub const MAX_HANDLERS: usize = 64;
pub const DEFAULT_TRANSITIONS: u16 = 64;
pub const DEFAULT_ACTIONS: u16 = 256;

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
                (kind @ ("sentinel" | "send" | "cron"), Some(target)) => {
                    Route::parse(&format!("{kind}:{target}"))?
                }
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

            if handler.id.is_empty()
                || handler.id.len() > 64
                || !handler
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                || !ids.insert(&handler.id)
            {
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

impl Store {
    pub fn accept_transition(
        &mut self,
        job: &Sentinel,
        bucket: &str,
        id: &str,
    ) -> Result<bool, String> {
        self.ensure_available()?;
        if id.is_empty() || id.len() > 128 || !job.buckets.iter().any(|declared| declared == bucket)
        {
            return Err("transition requires a stable identity and declared bucket".into());
        }
        let Some(current) = self.get(&job.id) else {
            return Ok(false);
        };
        if current
            .transitions
            .iter()
            .any(|transition| transition.id == id)
        {
            return Ok(false);
        }
        if !current.same_owner(job)
            || current.revision != job.revision
            || current.machine.stopped
            || current.machine.paused
            || current
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
        {
            return Ok(false);
        }
        if current.transitions.len() >= usize::from(current.max_transitions) {
            self.stop(&job.id, "transition_budget_exhausted")?;
            return Ok(false);
        }
        let mut accepted = job.clone();
        accepted.transitions = current.transitions.clone();
        accepted.transitions.push(Transition {
            id: id.into(),
            bucket: bucket.into(),
            accepted_at: self.clock.now_ms(),
            terminal: job.machine.stopped,
            handlers: Vec::new(),
        });
        self.finish(&accepted)
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

    pub fn claim_handler(
        &mut self,
        job: &Sentinel,
        transition_id: &str,
        handler: &Handler,
    ) -> Result<Option<ResultRecord>, String> {
        if !self.batch_live(job, transition_id) {
            return Ok(None);
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
            return Ok(None);
        }
        if !current
            .routes
            .get(&transition.bucket)
            .is_some_and(|handlers| handlers.contains(handler))
        {
            return Err("handler is not declared for the accepted transition".into());
        }
        let actions = current
            .transitions
            .iter()
            .flat_map(|t| &t.handlers)
            .filter(|r| r.claim_id.is_some())
            .count();
        if actions >= usize::from(current.max_actions) {
            self.stop(&job.id, "action_budget_exhausted")?;
            return Ok(None);
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
        let previous = self.jobs.clone();
        let current = self
            .jobs
            .iter_mut()
            .find(|j| j.id == job.id)
            .ok_or("sentinel disappeared")?;
        current
            .transitions
            .iter_mut()
            .find(|t| t.id == transition_id)
            .ok_or("transition missing")?
            .handlers
            .push(claim.clone());
        if let Err(error) = self.save() {
            self.jobs = previous;
            if let Some(cancelled) = self.active.get(&job.id) {
                cancelled.store(true, std::sync::atomic::Ordering::SeqCst)
            }
            return Err(error);
        }
        Ok(Some(claim))
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

    pub fn finish_handler_result(
        &mut self,
        source: &str,
        transition_id: &str,
        claim: &ResultRecord,
        effect: Effect,
    ) -> Result<(), String> {
        let previous = self.jobs.clone();
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
            return Ok(());
        };
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
        if let Err(error) = self.save() {
            self.jobs = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn skip_remaining(
        &mut self,
        source: &str,
        transition_id: &str,
        handlers: &[Handler],
    ) -> Result<(), String> {
        let previous = self.jobs.clone();
        let Some(transition) = self
            .jobs
            .iter_mut()
            .find(|j| j.id == source)
            .and_then(|j| j.transitions.iter_mut().find(|t| t.id == transition_id))
        else {
            return Ok(());
        };
        for handler in handlers {
            if !transition
                .handlers
                .iter()
                .any(|r| r.handler_id == handler.id)
            {
                transition.handlers.push(ResultRecord {
                    handler_id: handler.id.clone(),
                    status: Status::Skipped,
                    claim_id: None,
                    execution_id: None,
                    accepted_at: None,
                    command_result: None,
                    reason: Some("batch_fenced".into()),
                });
            }
        }
        if let Err(error) = self.save() {
            self.jobs = previous;
            return Err(error);
        }
        Ok(())
    }
}
