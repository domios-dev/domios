use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use codeterm_types::decision::{DecisionAnswer, DecisionError};
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::journal::{ActionRecord, Journal, VerdictKind};

const HOUR: i64 = 3_600_000;
const DAY: i64 = 86_400_000;

#[derive(Default)]
struct Activity {
    calls: VecDeque<i64>,
    last_activity: Option<i64>,
    last_verdict: BTreeMap<String, Value>,
    budget_reached: bool,
    storage_ready: bool,
}

pub(crate) struct SupporterRuntime {
    activity: Mutex<Activity>,
    journal: Mutex<Journal>,
}

impl SupporterRuntime {
    pub(crate) fn new(dir: PathBuf) -> Self {
        let journal = Journal::new(dir);
        let loaded = journal.load_calls();
        let storage_ready = loaded.is_ok();
        let snapshot = loaded.unwrap_or_default();
        let last_activity = snapshot.calls.back().copied();
        Self {
            activity: Mutex::new(Activity {
                calls: snapshot.calls,
                budget_reached: snapshot.budget_reached,
                last_activity,
                storage_ready,
                ..Default::default()
            }),
            journal: Mutex::new(journal),
        }
    }

    fn append(&self, value: Value) -> std::io::Result<()> {
        let result = self.journal.lock().append(&value);
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
        let reached = hour.len() >= cap;
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
                "time": now / 1000, "kind": "budget_transition", "tab": null,
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
        let mut state = self.activity.lock();
        let (hour, _) = self.update_budget(&mut state, cap, now);
        if hour >= cap {
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
            .save_calls(&state.calls, hour + 1 >= cap)
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
            "tab": tab, "question_count": questions, "action": "request", "reason": "connected" }),
        )
        .map_err(|_| DecisionError::Unavailable("journal_unavailable".into()))?;
        self.update_budget(&mut state, cap, now);
        Ok(())
    }

    pub(crate) fn verdict(
        &self,
        kind: VerdictKind,
        tab: Option<&str>,
        answer: &DecisionAnswer,
        model_id: Option<String>,
        criteria_version: &str,
        state_hash: &str,
    ) {
        let now = chrono::Utc::now().timestamp_millis();
        let (label, confidence) = match answer {
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
        };
        let value = json!({ "time": now / 1000, "kind": kind, "tab": tab, "label": label,
            "confidence": confidence, "action": "verdict", "reason": "model_answer",
            "distribution": answer, "model_id": model_id, "criteria_version": criteria_version,
            "state_hash": state_hash, "question_count": 1 });
        let mut state = self.activity.lock();
        state.last_activity = Some(now);
        state.last_verdict.insert(kind.name().into(), value.clone());
        let _ = self.append(value);
    }

    pub(crate) fn action(&self, record: ActionRecord<'_>) {
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
        self.journal.lock().claim_notification(key)
    }

    pub(crate) fn status(&self, cap: usize) -> Value {
        self.status_at(cap, chrono::Utc::now().timestamp_millis())
    }

    fn status_at(&self, cap: usize, now: i64) -> Value {
        let mut state = self.activity.lock();
        let (hour, free_at) = self.update_budget(&mut state, cap, now);
        json!({ "last_activity": state.last_activity.map(|time| time / 1000), "calls_last_hour": hour,
            "calls_last_day": state.calls.len(), "last_verdict": state.last_verdict,
            "budget": { "reached": state.budget_reached, "calls_per_hour": cap, "free_at": free_at.map(|time| (time + 999) / 1000), "storage_ready": state.storage_ready } })
    }
}

#[cfg(test)]
mod tests;
