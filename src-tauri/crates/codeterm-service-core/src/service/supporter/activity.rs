use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use codeterm_decision::{DecisionAnswer, DecisionError, DecisionPricing, DecisionResponse};
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::journal::{ActionRecord, Journal, VerdictKind};
use super::spend::{SpendAlert, SpendBook};

type TabFieldResolver = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
type JournalSink = Arc<dyn Fn(Value) + Send + Sync>;

const HOUR: i64 = 3_600_000;
const DAY: i64 = 86_400_000;
const COLD_ADAPTER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) struct DecisionEvidence<'a> {
    pub(crate) decision_point: Option<&'a str>,
    pub(crate) model_id: Option<String>,
    pub(crate) criteria_version: &'a str,
    pub(crate) state_hash: &'a str,
    pub(crate) question_set_hash: &'a str,
    pub(crate) asked: Option<Vec<super::QuestionAnswer>>,
}

#[derive(Default)]
struct Activity {
    calls: VecDeque<i64>,
    last_activity: Option<i64>,
    last_verdict: BTreeMap<String, Value>,
    budget_reached: bool,
    storage_ready: bool,
    ready_adapters: HashSet<String>,
    token_acl_notices: HashSet<String>,
}

pub struct SupporterRuntime {
    activity: Mutex<Activity>,
    backoffs: Mutex<super::backoff::Backoffs>,
    journal: Mutex<Journal>,
    spend: SpendBook,
    team_resolver: OnceLock<TabFieldResolver>,
    tab_title_resolver: OnceLock<TabFieldResolver>,
    journal_sink: OnceLock<JournalSink>,
    alert_sink: OnceLock<Arc<dyn Fn(SpendAlert) + Send + Sync>>,
    connection_epoch: AtomicU64,
}

impl SupporterRuntime {
    pub fn new(dir: PathBuf) -> Self {
        let spend = SpendBook::new(dir.clone());
        let journal = Journal::new(dir);
        let loaded = journal.load_calls();
        let storage_ready = loaded.is_ok();
        let snapshot = loaded.unwrap_or_default();
        let last_activity = snapshot.calls.back().copied();
        Self {
            backoffs: Mutex::new(Default::default()),
            activity: Mutex::new(Activity {
                calls: snapshot.calls,
                budget_reached: snapshot.budget_reached,
                last_activity,
                storage_ready,
                ..Default::default()
            }),
            journal: Mutex::new(journal),
            spend,
            alert_sink: OnceLock::new(),
            team_resolver: OnceLock::new(),
            tab_title_resolver: OnceLock::new(),
            journal_sink: OnceLock::new(),
            connection_epoch: AtomicU64::new(0),
        }
    }

    pub(crate) fn attach_alert_sink(&self, sink: Arc<dyn Fn(SpendAlert) + Send + Sync>) {
        let _ = self.alert_sink.set(sink);
    }

    pub fn attach_visibility(&self, resolver: TabFieldResolver, sink: JournalSink) {
        let _ = self.team_resolver.set(resolver);
        let _ = self.journal_sink.set(sink);
    }

    pub(crate) fn attach_tab_titles(&self, resolver: TabFieldResolver) {
        let _ = self.tab_title_resolver.set(resolver);
    }

    fn team_id(&self, tab: Option<&str>) -> Option<String> {
        tab.and_then(|tab| self.team_resolver.get().and_then(|resolve| resolve(tab)))
    }

    pub fn feed(&self, limit: usize, team: Option<&str>) -> std::io::Result<Vec<Value>> {
        self.journal.lock().feed(limit, team)
    }

    fn alerts(&self, adapter: &str, alerts: Vec<SpendAlert>) {
        for alert in alerts {
            let _ = self.append(json!({ "time": chrono::Utc::now().timestamp(), "kind": "spend_alert",
                "adapter": adapter, "action": "notify", "reason": alert.level.as_str(), "title": alert.title,
                "event_key": alert.event_key }));
            if let Some(sink) = self.alert_sink.get() {
                sink(alert);
            }
        }
    }

    pub(crate) fn adapter_available(&self, adapter: &str) -> bool {
        self.backoffs.lock().available(
            adapter,
            self.connection_epoch(),
            codeterm_decision::deadline::now(),
        )
    }

    pub(crate) fn admit_adapter(&self, adapter: &str) -> Result<(), DecisionError> {
        if !self.adapter_available(adapter) {
            return Err(DecisionError::Unavailable("adapter_backoff".into()));
        }
        self.spend
            .admit(adapter, chrono::Utc::now().timestamp_millis())
    }

    pub(crate) fn decision_deadline(
        &self,
        adapter: &str,
        configured_ms: u64,
        now: std::time::Instant,
    ) -> std::time::Instant {
        let configured =
            std::time::Duration::from_millis(configured_ms.max(1)).min(COLD_ADAPTER_DEADLINE);
        let adapter_ready = self.activity.lock().ready_adapters.contains(adapter);
        now + if adapter_ready {
            configured
        } else {
            configured.max(COLD_ADAPTER_DEADLINE)
        }
    }

    pub(crate) fn mark_adapter_ready(&self, adapter: &str) {
        self.backoffs.lock().recovered(adapter);
        self.activity
            .lock()
            .ready_adapters
            .insert(adapter.to_owned());
    }

    pub(crate) fn token_acl_state_changed(&self, adapter: &str, invalid: bool) {
        let alert = {
            let mut activity = self.activity.lock();
            if invalid {
                activity.token_acl_notices.insert(adapter.to_owned()).then(|| {
                    super::spend::SpendAlert {
                        title: "Supporter token permissions are invalid; save the token again to resume decisions".into(),
                        level: codeterm_notification_core::contract::NotifyLevel::Failure,
                        event_key: format!("supporter:{adapter}:token-acl"),
                    }
                })
            } else {
                activity.token_acl_notices.remove(adapter);
                None
            }
        };
        if let Some(alert) = alert {
            self.alerts(adapter, vec![alert]);
        }
    }

    pub(crate) fn admit_request_budget(&self, cap: usize) -> Result<(), DecisionError> {
        if cap == 0 {
            return Ok(());
        }
        let oldest = chrono::Utc::now().timestamp_millis() - HOUR;
        let hour = self
            .activity
            .lock()
            .calls
            .iter()
            .filter(|time| **time > oldest)
            .count();
        if hour >= cap {
            Err(DecisionError::Budget)
        } else {
            Ok(())
        }
    }

    pub(crate) fn decision(&self, decision: Value) -> std::io::Result<()> {
        self.append(decision)
    }

    pub(crate) fn reserve_request(
        &self,
        cap: usize,
        kind: VerdictKind,
        tab: Option<&str>,
        questions: usize,
        adapter: &str,
    ) -> Result<(), DecisionError> {
        self.spend
            .admit(adapter, chrono::Utc::now().timestamp_millis())?;
        self.reserve(cap, kind, tab, questions)
    }

    pub(crate) fn failure_with_activity(
        &self,
        adapter: Option<&str>,
        kind: VerdictKind,
        tab: Option<&str>,
        error: &DecisionError,
        context: Option<&super::ActivityContext>,
    ) {
        if matches!(error, DecisionError::Unavailable(reason) if reason == "adapter_backoff") {
            return;
        }
        if let (Some(adapter), DecisionError::Unavailable(_)) = (adapter, error) {
            if !self.backoffs.lock().failed(
                adapter,
                self.connection_epoch(),
                codeterm_decision::deadline::now(),
            ) {
                return;
            }
        }
        let reason = match error {
            DecisionError::Budget => "budget",
            DecisionError::CreditExhausted | DecisionError::Http(402) => "credit_exhausted",
            DecisionError::Deadline => "deadline",
            _ => "unavailable",
        };
        let decision_point = context.map_or_else(
            || kind.name(),
            super::narrative::ActivityContext::decision_point,
        );
        let _ = self.append(json!({ "time": chrono::Utc::now().timestamp(),
            "kind": kind, "tab": tab, "label": "other", "action": "notify_only", "reason": reason,
            "decision_point": decision_point,
            "backoff": matches!(error, DecisionError::Unavailable(_)),
            "asked": context.map(|context| context.failure(reason)) }));
    }

    pub fn record_spend(
        &self,
        adapter: &str,
        price: Option<&DecisionPricing>,
        response: &Result<DecisionResponse, DecisionError>,
    ) {
        let usage = response.as_ref().ok().and_then(|response| response.usage);
        let name = price.map_or(adapter, |price| price.display_name.as_str());
        match self.spend.finish(
            adapter,
            name,
            usage,
            price,
            response.as_ref().err(),
            chrono::Utc::now().timestamp_millis(),
        ) {
            Ok(alerts) => {
                self.alerts(adapter, alerts);
                let _ = self.append(json!({ "time": chrono::Utc::now().timestamp(),
                    "kind": "spend_updated", "adapter": adapter }));
            }
            Err(_) => log::warn!("[supporter] spend persistence failed"),
        }
    }

    /// Bumped before the journal row on every connection mutation attempt; it
    /// wraps only after 2^64 mutations and never resets within a daemon run.
    pub fn connection_changed(&self) {
        self.connection_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.append(
            json!({ "time": chrono::Utc::now().timestamp(), "kind": "connection_changed" }),
        );
    }

    pub(crate) fn connection_epoch(&self) -> u64 {
        self.connection_epoch.load(Ordering::Acquire)
    }

    pub fn spend_status(&self, adapter: &str) -> Value {
        self.spend
            .status(adapter, chrono::Utc::now().timestamp_millis())
    }

    pub(crate) fn response(
        &self,
        kind: VerdictKind,
        tab: Option<&str>,
        response: &DecisionResponse,
        evidence: DecisionEvidence<'_>,
    ) {
        let (label, confidence) = if let Some(answer) = response.answers.get("label") {
            answer_signal(answer)
        } else {
            (
                None,
                response
                    .answers
                    .values()
                    .next()
                    .and_then(|answer| answer_signal(answer).1),
            )
        };
        let value = json!({ "time": chrono::Utc::now().timestamp(), "kind": kind, "tab": tab,
            "label": label, "confidence": confidence, "action": "verdict", "reason": "model_answer",
            "decision_point": evidence.decision_point,
            "distribution": journal_answers(&response.answers), "usage": response.usage, "model_id": evidence.model_id,
            "criteria_version": evidence.criteria_version, "state_hash": evidence.state_hash, "question_set_hash": evidence.question_set_hash,
            "question_count": response.answers.len(), "asked": evidence.asked });
        let value = if matches!(kind, VerdictKind::TabState) {
            super::super::supporter_tab_state::redact_decision_diagnostic(&value)
        } else {
            value
        };
        let mut state = self.activity.lock();
        state.last_activity = Some(chrono::Utc::now().timestamp_millis());
        state.last_verdict.insert(kind.name().into(), value.clone());
        drop(state);
        let _ = self.append(value);
    }

    pub(crate) fn heartbeat_event(&self, event: Value) -> std::io::Result<()> {
        self.append(event)
    }

    pub(crate) fn sentinel_event(&self, event: Value) -> std::io::Result<()> {
        self.append(event)
    }

    fn append(&self, mut value: Value) -> std::io::Result<()> {
        if value.get("team_id").is_none() {
            value["team_id"] = json!(self.team_id(value["tab"].as_str()));
        }
        if let Some(title) = value["tab"].as_str().and_then(|tab| {
            self.tab_title_resolver
                .get()
                .and_then(|resolve| resolve(tab))
        }) {
            value["tab_title"] = json!(super::narrative::scrub(
                &title,
                super::narrative::MAX_NAME_CHARS,
            ));
        }
        let result = {
            let journal = self.journal.lock();
            value["budget"] = journal.load_calls().ok().map_or(
                Value::Null,
                |snapshot| json!({ "reached": snapshot.budget_reached }),
            );
            journal.append(&value)
        };
        if result.is_ok() {
            if let Some(sink) = self.journal_sink.get() {
                sink(json!({ "time": value["time"], "team_id": value["team_id"] }));
            }
        }
        if result.is_err() {
            log::warn!("[supporter] journal write failed");
        }
        result
    }

    fn update_budget(&self, state: &mut Activity, cap: usize, now: i64) -> (usize, Option<i64>) {
        state.calls.retain(|time| *time > now - DAY);
        let mut hour: Vec<_> = state
            .calls
            .iter()
            .copied()
            .filter(|time| *time > now - HOUR)
            .collect();
        hour.sort_unstable();
        let reached = cap > 0 && hour.len() >= cap;
        let free_at = if reached {
            hour.get(hour.len().saturating_sub(cap))
                .map(|time| time + HOUR)
        } else {
            None
        };
        if reached != state.budget_reached {
            state.budget_reached = reached;
            state.last_activity = Some(now);
            if self
                .journal
                .lock()
                .save_calls(&state.calls, reached)
                .is_err()
            {
                log::warn!("[supporter] budget transition persistence failed");
            }
            let _ = self.append(json!({
                "time": now / 1000, "kind": "budget_transition", "tab": null, "team_id": null,
                "action": if reached { "budget_reached" } else { "budget_available" },
                "reason": "hourly_window", "free_at": free_at.map(|time| (time + 999) / 1000),
            }));
        }
        (hour.len(), free_at)
    }

    pub(crate) fn reserve(
        &self,
        cap: usize,
        kind: VerdictKind,
        tab: Option<&str>,
        questions: usize,
    ) -> Result<(), DecisionError> {
        self.reserve_at(
            cap,
            kind,
            tab,
            questions,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    fn reserve_at(
        &self,
        cap: usize,
        kind: VerdictKind,
        tab: Option<&str>,
        questions: usize,
        now: i64,
    ) -> Result<(), DecisionError> {
        let team_id = self.team_id(tab);
        let mut state = self.activity.lock();
        let (hour, _) = self.update_budget(&mut state, cap, now);
        if cap > 0 && hour >= cap {
            return Err(DecisionError::Budget);
        }
        if !state.storage_ready {
            return Err(DecisionError::Unavailable(
                "budget_storage_unavailable".into(),
            ));
        }
        state.calls.push_back(now);
        if self
            .journal
            .lock()
            .save_calls(&state.calls, cap > 0 && hour + 1 >= cap)
            .is_err()
        {
            state.calls.pop_back();
            return Err(DecisionError::Unavailable(
                "budget_storage_unavailable".into(),
            ));
        }
        state.last_activity = Some(now);
        self.append(
            json!({ "time": now / 1000, "kind": "model_call", "verdict_kind": kind,
            "tab": tab, "team_id": team_id, "question_count": questions, "action": "request", "reason": "connected" }),
        )
        .map_err(|_| DecisionError::Unavailable("journal_unavailable".into()))?;
        self.update_budget(&mut state, cap, now);
        Ok(())
    }

    pub(crate) fn provider_update_action(
        &self,
        provider: &str,
        target: Option<&str>,
        record: ActionRecord<'_>,
    ) -> std::io::Result<()> {
        let mut value = serde_json::to_value(record)?;
        let now = chrono::Utc::now().timestamp_millis();
        value["time"] = json!(now / 1000);
        value["provider"] = json!(provider);
        value["target_version"] = json!(target);
        self.activity.lock().last_activity = Some(now);
        self.append(value)
    }

    pub fn action(&self, record: ActionRecord<'_>) {
        let now = chrono::Utc::now().timestamp_millis();
        let mut value = serde_json::to_value(record).unwrap_or(Value::Null);
        value["time"] = json!(now / 1000);
        self.activity.lock().last_activity = Some(now);
        let _ = self.append(value);
    }

    pub(crate) fn try_action(&self, record: ActionRecord<'_>) -> std::io::Result<()> {
        let mut value = serde_json::to_value(record)?;
        let now = chrono::Utc::now().timestamp_millis();
        value["time"] = json!(now / 1000);
        self.activity.lock().last_activity = Some(now);
        self.append(value)
    }

    pub(crate) fn claim_notification(&self, key: &str) -> std::io::Result<bool> {
        let result = self.journal.lock().claim_notification(key);
        if matches!(result, Ok(true)) {
            if let Some(sink) = self.journal_sink.get() {
                sink(json!({ "team_id": null }));
            }
        }
        result
    }

    pub fn status(&self, cap: usize) -> Value {
        self.status_at(cap, chrono::Utc::now().timestamp_millis())
    }

    fn status_at(&self, cap: usize, now: i64) -> Value {
        let mut state = self.activity.lock();
        let (hour, free_at) = self.update_budget(&mut state, cap, now);
        json!({ "last_activity": state.last_activity.map(|time| time / 1000), "calls_last_hour": hour,
            "calls_last_day": state.calls.len(), "last_verdict": state.last_verdict,
            "budget": { "reached": state.budget_reached, "calls_per_hour": (cap > 0).then_some(cap), "free_at": free_at.map(|time| (time + 999) / 1000), "storage_ready": state.storage_ready } })
    }
}

fn journal_answers(
    answers: &codeterm_decision::DecisionAnswers,
) -> codeterm_decision::DecisionAnswers {
    answers
        .iter()
        .map(|(name, answer)| {
            let answer = match answer {
                DecisionAnswer::Choice {
                    choice,
                    probabilities,
                    confidence,
                } if name != "label" => DecisionAnswer::Choice {
                    choice: super::criteria_hash(choice.as_bytes()),
                    probabilities: probabilities
                        .iter()
                        .map(|(option, p)| (super::criteria_hash(option.as_bytes()), *p))
                        .collect(),
                    confidence: *confidence,
                },
                answer => answer.clone(),
            };
            (name.clone(), answer)
        })
        .collect()
}

fn answer_signal(answer: &DecisionAnswer) -> (Option<&str>, Option<f64>) {
    match answer {
        DecisionAnswer::Choice {
            choice,
            probabilities,
            confidence,
        } => (
            Some(choice.as_str()),
            confidence.or_else(|| probabilities.get(choice).copied()),
        ),
        DecisionAnswer::Noul { p } => (None, Some(*p)),
        DecisionAnswer::Score { confidence, .. } => (None, *confidence),
    }
}

#[cfg(test)]
mod tests;
