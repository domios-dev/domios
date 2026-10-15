use std::collections::BTreeMap;
use std::path::PathBuf;

use codeterm_decision::{DecisionError, DecisionPricing, DecisionUsage};
use codeterm_notification_core::contract::NotifyLevel;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::storage;

const REPROBE_MS: i64 = 3_600_000;

#[derive(Clone, Debug)]
pub(crate) struct SpendAlert {
    pub(crate) title: String,
    pub(crate) level: NotifyLevel,
    pub(crate) event_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Totals {
    input_tokens: u64,
    output_tokens: u64,
    estimated_usd: Option<f64>,
}

impl Default for Totals {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            estimated_usd: Some(0.0),
        }
    }
}

impl Totals {
    fn add(&mut self, usage: DecisionUsage, price: Option<&DecisionPricing>) {
        self.input_tokens = self.input_tokens.saturating_add(usage.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(usage.output_tokens);
        self.estimated_usd = self
            .estimated_usd
            .zip(price.and_then(|p| p.estimated_usd(usage)))
            .map(|(a, b)| a + b)
            .filter(|value| value.is_finite());
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Account {
    day: String,
    month: String,
    today: Totals,
    monthly: Totals,
    exhausted: bool,
    exhausted_alerted: bool,
    next_probe_at: Option<i64>,
}

impl Account {
    fn rollover(&mut self, now: i64) {
        let date = chrono::DateTime::from_timestamp_millis(now).unwrap_or_default();
        let day = date.format("%Y-%m-%d").to_string();
        let month = date.format("%Y-%m").to_string();
        if self.day != day {
            self.day = day;
            self.today = Totals::default();
        }
        if self.month != month {
            self.month = month;
            self.monthly = Totals::default();
        }
    }
}

struct State {
    accounts: BTreeMap<String, Account>,
    storage_ready: bool,
}

pub(super) struct SpendBook {
    path: PathBuf,
    state: Mutex<State>,
}

impl SpendBook {
    pub(super) fn new(dir: PathBuf) -> Self {
        let path = dir.join("spend.json");
        let loaded = storage::load(&path);
        Self {
            path,
            state: Mutex::new(State {
                storage_ready: loaded.is_ok(),
                accounts: loaded.unwrap_or_default(),
            }),
        }
    }

    fn persist(&self, state: &mut State) -> Result<(), DecisionError> {
        if !state.storage_ready || storage::save(&self.path, &state.accounts).is_err() {
            state.storage_ready = false;
            return Err(DecisionError::Unavailable(
                "spend_storage_unavailable".into(),
            ));
        }
        Ok(())
    }

    pub fn admit(&self, adapter: &str, now: i64) -> Result<(), DecisionError> {
        let mut state = self.state.lock();
        if !state.storage_ready {
            return Err(DecisionError::Unavailable(
                "spend_storage_unavailable".into(),
            ));
        }
        let account = state.accounts.entry(adapter.into()).or_default();
        if account.exhausted {
            if account.next_probe_at.is_none_or(|at| now < at) {
                return Err(DecisionError::CreditExhausted);
            }
            account.next_probe_at = Some(now + REPROBE_MS);
            self.persist(&mut state)?;
        }
        Ok(())
    }

    pub fn finish(
        &self,
        adapter: &str,
        name: &str,
        usage: Option<DecisionUsage>,
        price: Option<&DecisionPricing>,
        error: Option<&DecisionError>,
        now: i64,
    ) -> Result<Vec<SpendAlert>, DecisionError> {
        let mut state = self.state.lock();
        let account = state.accounts.entry(adapter.into()).or_default();
        account.rollover(now);
        let mut alerts = Vec::new();
        if price.is_none() || (usage.is_none() && error.is_none()) {
            account.today.estimated_usd = None;
            account.monthly.estimated_usd = None;
        }
        if let Some(usage) = usage {
            account.today.add(usage, price);
            account.monthly.add(usage, price);
        }
        if error.is_none() {
            account.exhausted = false;
            account.exhausted_alerted = false;
            account.next_probe_at = None;
        } else if matches!(
            error,
            Some(DecisionError::CreditExhausted | DecisionError::Http(402))
        ) {
            account.exhausted = true;
            account.next_probe_at = Some(now + REPROBE_MS);
            if !account.exhausted_alerted {
                account.exhausted_alerted = true;
                alerts.push(SpendAlert {
                    title: format!(
                        "{name} is out of credit; Supporter is using deterministic results"
                    ),
                    level: NotifyLevel::Failure,
                    event_key: format!("supporter:{adapter}:exhausted:{now}"),
                });
            }
        }
        self.persist(&mut state)?;
        Ok(alerts)
    }

    pub fn status(&self, adapter: &str, now: i64) -> Value {
        let state = self.state.lock();
        let mut account = state.accounts.get(adapter).cloned().unwrap_or_default();
        account.rollover(now);
        json!({ "today": account.today, "month": account.monthly, "exhausted": account.exhausted,
            "next_probe_at": account.next_probe_at.map(|at| (at + 999) / 1000), "storage_ready": state.storage_ready })
    }
}

#[cfg(test)]
mod tests;
