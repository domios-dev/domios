use super::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobQuote {
    pub binding: String,
    pub amount: u64,
    pub unit: String,
    pub basis: String,
    pub tariff_revision: String,
    pub issued_at: u64,
    pub valid_until_ms: u64,
}
impl JobQuote {
    pub fn valid(&self, now: u64) -> bool {
        crate::media_delivery::valid_token(&self.binding)
            && self.amount > 0
            && self.amount <= MAX_COST_AMOUNT
            && !self.unit.is_empty()
            && self.unit.len() <= 64
            && !self.basis.is_empty()
            && self.basis.len() <= 512
            && !self.tariff_revision.is_empty()
            && self.tariff_revision.len() <= 128
            && self.issued_at <= now
            && self.valid_until_ms > now
            && self.valid_until_ms.saturating_sub(self.issued_at) <= 300_000
            && [&self.unit, &self.basis, &self.tariff_revision]
                .iter()
                .all(|v| !v.chars().any(char::is_control))
    }
}

pub fn job_request_binding(request: &serde_json::Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(request).map_err(|_| "invalid job request")?;
    if !request.is_object() || bytes.len() > 2048 {
        return Err("bounded job request object required".into());
    }
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobSpendState {
    WaitingForOwner,
    ApprovedOnce,
    ApprovedBudget,
    Denied,
    EnteredUnknown,
    ExpiredReadmitNeeded,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobSpendReceipt {
    pub operation: String,
    pub scope: PluginJobScope,
    pub origin: PluginJobOrigin,
    pub quote: JobQuote,
    pub request: serde_json::Value,
    pub state: JobSpendState,
    pub daily_allowance: u64,
    pub entered_at_ms: Option<u64>,
}
impl JobSpendReceipt {
    pub fn at(mut self, now: u64) -> Self {
        if self.quote.valid_until_ms <= now
            && matches!(
                self.state,
                JobSpendState::WaitingForOwner
                    | JobSpendState::ApprovedOnce
                    | JobSpendState::ApprovedBudget
            )
        {
            self.state = JobSpendState::ExpiredReadmitNeeded;
        }
        self
    }
}
fn error(_: rusqlite::Error) -> String {
    "job spend persistence failed".into()
}
fn scope_key(scope: &PluginJobScope) -> Result<String, String> {
    serde_json::to_string(scope).map_err(|_| "invalid job scope".into())
}
fn read(
    db: &rusqlite::Connection,
    plugin: &str,
    operation: &str,
) -> Result<Option<JobSpendReceipt>, String> {
    let record: Option<String> = db
        .query_row(
            "SELECT record FROM plugin_job_spend WHERE operation=?1 AND plugin=?2",
            params![operation, plugin],
            |r| r.get(0),
        )
        .optional()
        .map_err(error)?;
    record
        .map(|v| serde_json::from_str(&v).map_err(|_| "invalid job spend receipt".into()))
        .transpose()
}
fn write(db: &rusqlite::Connection, receipt: &JobSpendReceipt) -> Result<(), String> {
    db.execute("INSERT OR REPLACE INTO plugin_job_spend(operation,plugin,scope,unit,reserved,record) VALUES(?1,?2,?3,?4,?5,?6)",params![receipt.operation,receipt.scope.plugin,scope_key(&receipt.scope)?,receipt.quote.unit,if receipt.state == JobSpendState::EnteredUnknown { receipt.quote.amount } else { 0 },serde_json::to_string(receipt).map_err(|_| "invalid job spend receipt")?]).map_err(error)?;
    Ok(())
}
fn remaining(
    db: &rusqlite::Connection,
    scope: &PluginJobScope,
    unit: &str,
    now: u64,
) -> Result<u64, String> {
    let day = now / 86_400_000;
    db.execute(
        "UPDATE plugin_job_budgets SET period=?3 WHERE scope=?1 AND unit=?2 AND period<?3",
        params![scope_key(scope)?, unit, day],
    )
    .map_err(error)?;
    let budget: u64 = db
        .query_row(
            "SELECT amount FROM plugin_job_budgets WHERE scope=?1 AND unit=?2 AND period=?3",
            params![scope_key(scope)?, unit, day],
            |r| r.get(0),
        )
        .optional()
        .map_err(error)?
        .unwrap_or(0);
    let reserved: u64 = db
        .query_row(
            "SELECT COALESCE(SUM(reserved),0) FROM plugin_job_spend WHERE scope=?1 AND unit=?2 AND json_extract(record,'$.enteredAtMs')>=?3 AND json_extract(record,'$.enteredAtMs')<?4",
            params![scope_key(scope)?, unit, day * 86_400_000, day.saturating_add(1).saturating_mul(86_400_000)],
            |r| r.get(0),
        )
        .map_err(error)?;
    Ok(budget.saturating_sub(reserved))
}
impl OperationLedger {
    pub fn job_spend(
        &self,
        plugin: &str,
        operation: &str,
    ) -> Result<Option<JobSpendReceipt>, String> {
        read(&self.inner.db.lock(), plugin, operation)
    }
    pub fn prepare_job_spend(
        &self,
        origin: &JobOriginReceipt,
        request: &serde_json::Value,
        quote: JobQuote,
        now: u64,
    ) -> Result<JobSpendReceipt, String> {
        if quote.valid_until_ms <= now {
            if let Some(existing) = self.job_spend(&origin.scope.plugin, &origin.operation)? {
                if existing.quote == quote
                    && existing.scope == origin.scope
                    && existing.origin == origin.origin
                    && quote.binding == job_request_binding(request)?
                {
                    return Ok(existing.at(now));
                }
            }
        }
        if origin.deadline_ms <= now
            || !quote.valid(now)
            || quote.binding != job_request_binding(request)?
        {
            if quote.valid_until_ms <= now {
                return Err("quote expired; re-admit needed".into());
            }
            return Err("fresh exact request quote required".into());
        }
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(error)?;
        if let Some(existing) = read(&tx, &origin.scope.plugin, &origin.operation)? {
            if existing.quote != quote {
                return Err("job quote cannot be replaced".into());
            }
            return Ok(existing);
        }
        let state = if remaining(&tx, &origin.scope, &quote.unit, now)? >= quote.amount {
            JobSpendState::ApprovedBudget
        } else {
            JobSpendState::WaitingForOwner
        };
        let receipt = JobSpendReceipt {
            operation: origin.operation.clone(),
            scope: origin.scope.clone(),
            origin: origin.origin.clone(),
            request: request.clone(),
            daily_allowance: quote.amount.saturating_mul(10).min(MAX_COST_AMOUNT),
            quote,
            state,
            entered_at_ms: None,
        };
        write(&tx, &receipt)?;
        tx.commit().map_err(error)?;
        Ok(receipt)
    }
    pub fn decide_job_spend(
        &self,
        plugin: &str,
        operation: &str,
        decision: &str,
        now: u64,
    ) -> Result<JobSpendReceipt, String> {
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(error)?;
        let mut receipt = read(&tx, plugin, operation)?.ok_or("job quote unavailable")?;
        if receipt.quote.valid_until_ms <= now {
            return Err("quote expired; re-admit needed".into());
        }
        if receipt.state != JobSpendState::WaitingForOwner || !receipt.quote.valid(now) {
            return Err("job quote no longer awaiting approval".into());
        }
        receipt.state = match decision {
            "Approve once" => JobSpendState::ApprovedOnce,
            "Set daily allowance" => {
                tx.execute("INSERT OR REPLACE INTO plugin_job_budgets(scope,unit,amount,period) VALUES(?1,?2,?3,?4)",params![scope_key(&receipt.scope)?,receipt.quote.unit,receipt.daily_allowance,now / 86_400_000]).map_err(error)?;
                JobSpendState::ApprovedOnce
            }
            "Decline" => JobSpendState::Denied,
            _ => return Err("invalid job spend decision".into()),
        };
        write(&tx, &receipt)?;
        tx.commit().map_err(error)?;
        Ok(receipt)
    }
    pub fn enter_job_spend(
        &self,
        origin: &JobOriginReceipt,
        request: &serde_json::Value,
        now: u64,
    ) -> Result<JobSpendReceipt, String> {
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(error)?;
        let mut receipt =
            read(&tx, &origin.scope.plugin, &origin.operation)?.ok_or("job quote unavailable")?;
        if receipt.scope != origin.scope
            || receipt.origin != origin.origin
            || origin.deadline_ms <= now
            || receipt.quote.binding != job_request_binding(request)?
        {
            return Err("job spend scope or quote changed".into());
        }
        let projected = receipt.clone().at(now);
        if projected.state == JobSpendState::ExpiredReadmitNeeded {
            return Ok(projected);
        }
        if !receipt.quote.valid(now) {
            return Err("fresh exact request quote required".into());
        }
        match receipt.state {
            JobSpendState::ApprovedOnce => (),
            JobSpendState::ApprovedBudget
                if remaining(&tx, &receipt.scope, &receipt.quote.unit, now)?
                    >= receipt.quote.amount =>
            {
                ()
            }
            JobSpendState::ApprovedBudget => {
                receipt.state = JobSpendState::WaitingForOwner;
                write(&tx, &receipt)?;
                tx.commit().map_err(error)?;
                return Ok(receipt);
            }
            _ => return Err("job spend not authorized or already entered".into()),
        }
        receipt.state = JobSpendState::EnteredUnknown;
        receipt.entered_at_ms = Some(now);
        write(&tx, &receipt)?;
        tx.commit().map_err(error)?;
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests;
