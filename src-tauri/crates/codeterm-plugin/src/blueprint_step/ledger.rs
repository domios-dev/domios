use super::*;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub struct OperationLedger {
    db: Mutex<Connection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRecord {
    pub plugin: String,
    pub request: StepRequest,
    pub state: OperationState,
    pub job: Option<String>,
    pub waiting: Option<Waiting>,
    pub reserved: u64,
    pub authorized: Option<u64>,
    pub outcome: Option<String>,
    pub outputs: PortValues,
}

fn db_error(_: rusqlite::Error) -> String {
    "plugin operation persistence failed".into()
}

impl OperationLedger {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        Self::from_connection(Connection::open(path).map_err(db_error)?)
    }

    pub fn from_connection(db: Connection) -> Result<Self, String> {
        db.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS blueprint_plugin_runs (scope TEXT NOT NULL, unit TEXT NOT NULL, ceiling INTEGER NOT NULL, PRIMARY KEY(scope,unit));
            CREATE TABLE IF NOT EXISTS blueprint_plugin_operations (id TEXT PRIMARY KEY, scope TEXT NOT NULL, unit TEXT NOT NULL, reserved INTEGER NOT NULL, record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS blueprint_plugin_operation_scope ON blueprint_plugin_operations(scope);").map_err(db_error)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn admit_run(&self, scope: &RunScope, unit: &str, ceiling: u64) -> Result<(), String> {
        if !token(unit) || ceiling > MAX_COST_AMOUNT {
            return Err("invalid spending authorization".into());
        }
        let key = serde_json::to_string(scope).map_err(|_| "invalid scope")?;
        let db = self.db.lock();
        db.execute(
            "INSERT OR IGNORE INTO blueprint_plugin_runs VALUES (?1,?2,?3)",
            params![key, unit, ceiling],
        )
        .map_err(db_error)?;
        let existing: (String, u64) = db
            .query_row(
                "SELECT unit,ceiling FROM blueprint_plugin_runs WHERE scope=?1 AND unit=?2",
                params![key, unit],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_error)?;
        if existing != (unit.into(), ceiling) {
            return Err("run authorization is immutable".into());
        }
        Ok(())
    }

    pub fn prepare(&self, plugin: &str, request: &StepRequest) -> Result<OperationRecord, String> {
        let key = serde_json::to_string(&request.context.scope).map_err(|_| "invalid scope")?;
        let mut db = self.db.lock();
        let tx = db.transaction().map_err(db_error)?;
        if let Some(existing) = read(&tx, &request.context.operation)? {
            let mut expected = request.clone();
            expected.context.attempt = existing.request.context.attempt.clone();
            if existing.plugin != plugin || existing.request != expected {
                return Err("operation identity collision".into());
            }
            return Ok(existing);
        }
        let count: u64 = tx
            .query_row(
                "SELECT count(*) FROM blueprint_plugin_operations WHERE scope=?1",
                [&key],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if count >= 64 {
            return Err("run exceeds 64 plugin operations".into());
        }
        let record = OperationRecord {
            plugin: plugin.into(),
            request: request.clone(),
            state: OperationState::Prepared,
            job: None,
            waiting: None,
            reserved: 0,
            authorized: None,
            outcome: None,
            outputs: Default::default(),
        };
        tx.execute(
            "INSERT INTO blueprint_plugin_operations VALUES (?1,?2,?3,0,?4)",
            params![
                request.context.operation,
                key,
                request.unit,
                encode(&record)?
            ],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(record)
    }

    pub fn get(&self, operation: &str) -> Result<Option<OperationRecord>, String> {
        read(&self.db.lock(), operation)
    }

    pub(crate) fn run_unit(&self, scope: &RunScope, unit: &str) -> Result<String, String> {
        let key = serde_json::to_string(scope).map_err(|_| "invalid scope")?;
        self.db
            .lock()
            .query_row(
                "SELECT unit FROM blueprint_plugin_runs WHERE scope=?1 AND unit=?2",
                params![key, unit],
                |r| r.get(0),
            )
            .map_err(|_| "run spending authorization missing".into())
    }

    pub fn history(&self, scope: &RunScope) -> Result<Vec<OperationRecord>, String> {
        let key = serde_json::to_string(scope).map_err(|_| "invalid scope")?;
        let db = self.db.lock();
        let mut query = db.prepare("SELECT record FROM blueprint_plugin_operations WHERE scope=?1 ORDER BY rowid LIMIT 64").map_err(db_error)?;
        let rows = query
            .query_map([key], |r| r.get::<_, String>(0))
            .map_err(db_error)?;
        rows.map(|row| decode(&row.map_err(db_error)?)).collect()
    }

    pub fn authorize(
        &self,
        operation: &str,
        scope: &RunScope,
        actor: &str,
        allowance: u64,
    ) -> Result<(), String> {
        if allowance > MAX_COST_AMOUNT {
            return Err("allowance exceeds ledger bounds".into());
        }
        self.change(operation, |record| {
            if record.state != OperationState::WaitingForOwnerOk
                || &record.request.context.scope != scope
                || record.request.context.owner != actor
            {
                return Err("owner authorization refused".into());
            }
            record.authorized = Some(allowance);
            Ok(())
        })
    }

    pub(crate) fn reserve(
        &self,
        operation: &str,
        estimate: Option<Estimate>,
        now_ms: u64,
    ) -> Result<OperationRecord, String> {
        let mut db = self.db.lock();
        let tx = db.transaction().map_err(db_error)?;
        let mut record = read(&tx, operation)?.ok_or("operation missing")?;
        if !matches!(
            record.state,
            OperationState::Prepared | OperationState::WaitingForOwnerOk
        ) {
            return Err("operation already crossed submission boundary".into());
        }
        let key =
            serde_json::to_string(&record.request.context.scope).map_err(|_| "invalid scope")?;
        let (unit, ceiling): (String, u64) = tx
            .query_row(
                "SELECT unit,ceiling FROM blueprint_plugin_runs WHERE scope=?1 AND unit=?2",
                params![key, record.request.unit],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "run spending authorization missing")?;
        let used: u64 = tx.query_row("SELECT coalesce(sum(reserved),0) FROM blueprint_plugin_operations WHERE scope=?1 AND unit=?2", params![key, unit], |r| r.get(0)).map_err(db_error)?;
        let remaining = ceiling.saturating_sub(used);
        let amount = estimate.as_ref().map(|e| e.amount);
        let reason = match estimate.as_ref() {
            None => Some(WaitingReason::EstimateMissing),
            Some(e)
                if e.unit != unit || e.valid_until_ms <= now_ms || e.amount > MAX_COST_AMOUNT =>
            {
                Some(WaitingReason::EstimateStale)
            }
            Some(e) if record.authorized.is_some_and(|max| e.amount > max) => {
                Some(WaitingReason::OverStepAllowance)
            }
            Some(_) if record.authorized.is_some() => None,
            Some(e) if e.amount > record.request.step_allowance => {
                Some(WaitingReason::OverStepAllowance)
            }
            Some(e) if e.amount > remaining => Some(WaitingReason::OverRunCeiling),
            Some(_) => None,
        };
        if let Some(reason) = reason {
            record.state = OperationState::WaitingForOwnerOk;
            record.waiting = Some(Waiting {
                reason,
                estimate: amount,
                remaining,
                unit,
            });
        } else {
            record.state = OperationState::Submitting;
            record.reserved = amount.ok_or("estimate missing")?;
            record.waiting = None;
        }
        write(&tx, &record)?;
        tx.commit().map_err(db_error)?;
        Ok(record)
    }

    pub(crate) fn change(
        &self,
        operation: &str,
        update: impl FnOnce(&mut OperationRecord) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut db = self.db.lock();
        let tx = db.transaction().map_err(db_error)?;
        let mut record = read(&tx, operation)?.ok_or("operation missing")?;
        update(&mut record)?;
        write(&tx, &record)?;
        tx.commit().map_err(db_error)
    }
}

fn encode(record: &OperationRecord) -> Result<String, String> {
    let value = serde_json::to_string(record).map_err(|_| "invalid operation record")?;
    if value.len() > 3 * MAX_STEP_BYTES {
        return Err("operation record exceeds bound".into());
    }
    Ok(value)
}
fn decode(value: &str) -> Result<OperationRecord, String> {
    serde_json::from_str(value).map_err(|_| "invalid persisted operation".into())
}
fn read(db: &Connection, id: &str) -> Result<Option<OperationRecord>, String> {
    db.query_row(
        "SELECT record FROM blueprint_plugin_operations WHERE id=?1",
        [id],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(db_error)?
    .map(|v| decode(&v))
    .transpose()
}
fn write(db: &Connection, record: &OperationRecord) -> Result<(), String> {
    db.execute(
        "UPDATE blueprint_plugin_operations SET reserved=?2,record=?3 WHERE id=?1",
        params![
            record.request.context.operation,
            record.reserved,
            encode(record)?
        ],
    )
    .map_err(db_error)?;
    Ok(())
}
