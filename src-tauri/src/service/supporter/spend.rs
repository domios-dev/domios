use std::collections::BTreeMap;
use std::path::PathBuf;

use codeterm_types::decision::{DecisionError, DecisionPricing, DecisionUsage};
use codeterm_types::notification::NotifyLevel;
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
    since_balance: Totals,
    balance_usd: Option<f64>,
    balance_set_at: Option<i64>,
    entry: String,
    low_alerted: bool,
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

    fn remaining(&self) -> Option<f64> {
        self.balance_usd
            .zip(self.since_balance.estimated_usd)
            .map(|(a, b)| a - b)
    }

    fn low_alert(&mut self, name: &str) -> Option<SpendAlert> {
        if self.low_alerted || !self.remaining().is_some_and(|remaining| remaining < 1.0) {
            return None;
        }
        self.low_alerted = true;
        Some(SpendAlert {
            title: format!("{name} balance below $1 (estimate)"),
            level: NotifyLevel::ActionNeeded,
            event_key: format!("supporter:{name}:{}:low", self.entry),
        })
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

    pub(super) fn admit(&self, adapter: &str, now: i64) -> Result<(), DecisionError> {
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

    pub(super) fn finish(
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
            account.since_balance.estimated_usd = None;
        }
        if let Some(usage) = usage {
            account.today.add(usage, price);
            account.monthly.add(usage, price);
            account.since_balance.add(usage, price);
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
                        "{name} balance exhausted, Supporter falling back to deterministic"
                    ),
                    level: NotifyLevel::Failure,
                    event_key: format!("supporter:{adapter}:{}:exhausted:{now}", account.entry),
                });
            }
        }
        if let Some(alert) = account.low_alert(name) {
            alerts.push(alert);
        }
        self.persist(&mut state)?;
        Ok(alerts)
    }

    pub(super) fn set_balance(
        &self,
        adapter: &str,
        name: &str,
        price: Option<&DecisionPricing>,
        balance: Option<f64>,
        now: i64,
    ) -> Result<Vec<SpendAlert>, DecisionError> {
        if balance.is_some_and(|value| !value.is_finite() || value < 0.0) {
            return Err(DecisionError::Parse("invalid_balance".into()));
        }
        let mut state = self.state.lock();
        let account = state.accounts.entry(adapter.into()).or_default();
        account.balance_usd = balance;
        account.balance_set_at = balance.map(|_| now / 1000);
        account.since_balance = Totals::default();
        account.since_balance.estimated_usd =
            price.and_then(|price| price.estimated_usd(DecisionUsage::default()));
        account.entry = uuid::Uuid::new_v4().to_string();
        account.low_alerted = false;
        account.exhausted = false;
        account.exhausted_alerted = false;
        account.next_probe_at = None;
        let alerts = account.low_alert(name).into_iter().collect();
        self.persist(&mut state)?;
        Ok(alerts)
    }

    pub(super) fn status(&self, adapter: &str, now: i64) -> Value {
        let state = self.state.lock();
        let mut account = state.accounts.get(adapter).cloned().unwrap_or_default();
        account.rollover(now);
        json!({ "today": account.today, "month": account.monthly, "since_balance": account.since_balance,
            "balance_usd": account.balance_usd, "balance_set_at": account.balance_set_at,
            "remaining_usd": account.remaining(), "exhausted": account.exhausted,
            "next_probe_at": account.next_probe_at.map(|at| (at + 999) / 1000), "storage_ready": state.storage_ready })
    }
}

#[cfg(test)]
mod tests;
