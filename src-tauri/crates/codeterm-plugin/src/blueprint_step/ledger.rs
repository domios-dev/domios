use super::*;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Weak};

pub struct OperationLedger {
    pub(super) inner: Arc<LedgerInner>,
}

pub(super) struct LedgerInner {
    pub(super) db: Mutex<Connection>,
    active: Mutex<BTreeSet<String>>,
}

static OPEN_LEDGERS: LazyLock<Mutex<BTreeMap<PathBuf, Weak<LedgerInner>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

pub(crate) struct OperationLease {
    inner: Arc<LedgerInner>,
    operation: String,
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        self.inner.active.lock().remove(&self.operation);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationApproval {
    pub scope: RunScope,
    pub operation: String,
    pub actor: String,
    pub unit: String,
    pub maximum: u64,
    pub run_ceiling_exception: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRecord {
    pub plugin: String,
    pub request: StepRequest,
    pub state: OperationState,
    pub job: Option<String>,
    pub waiting: Option<Waiting>,
    #[serde(default)]
    pub estimate: Option<Estimate>,
    #[serde(default)]
    pub question_revision: u32,
    pub reserved: u64,
    pub authorized: Option<OperationApproval>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancel_requested: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancel_dispatched: bool,
    pub outcome: Option<String>,
    pub outputs: PortValues,
}

fn db_error(_: rusqlite::Error) -> String {
    "plugin operation persistence failed".into()
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl OperationLedger {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let db = Connection::open(path).map_err(db_error)?;
        let key = std::fs::canonicalize(path)
            .map_err(|_| "plugin operation store identity unavailable")?;
        let mut open = OPEN_LEDGERS.lock();
        open.retain(|_, ledger| ledger.strong_count() > 0);
        if let Some(inner) = open.get(&key).and_then(Weak::upgrade) {
            return Ok(Self { inner });
        }
        let ledger = Self::from_connection(db)?;
        open.insert(key, Arc::downgrade(&ledger.inner));
        Ok(ledger)
    }

    pub(crate) fn from_connection(db: Connection) -> Result<Self, String> {
        db.execute_batch("PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS blueprint_plugin_runs (scope TEXT NOT NULL, unit TEXT NOT NULL, ceiling INTEGER NOT NULL, PRIMARY KEY(scope,unit));
            CREATE TABLE IF NOT EXISTS blueprint_plugin_operations (id TEXT PRIMARY KEY, scope TEXT NOT NULL, unit TEXT NOT NULL, reserved INTEGER NOT NULL, record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS blueprint_plugin_operation_scope ON blueprint_plugin_operations(scope);
            CREATE TABLE IF NOT EXISTS blueprint_plugin_monitors (operation TEXT PRIMARY KEY,plugin TEXT NOT NULL,due INTEGER NOT NULL,deadline INTEGER NOT NULL,stopped INTEGER NOT NULL,claim_until INTEGER NOT NULL,record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS blueprint_plugin_monitor_due ON blueprint_plugin_monitors(stopped,due);").map_err(db_error)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS blueprint_job_origins(operation TEXT PRIMARY KEY,plugin TEXT NOT NULL,deadline INTEGER NOT NULL,record TEXT NOT NULL);").map_err(db_error)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS blueprint_run_views(scope TEXT PRIMARY KEY,blueprint_key TEXT NOT NULL,reference TEXT UNIQUE NOT NULL,ordinal INTEGER NOT NULL,record TEXT NOT NULL);").map_err(db_error)?;
        Ok(Self {
            inner: Arc::new(LedgerInner {
                db: Mutex::new(db),
                active: Mutex::new(BTreeSet::new()),
            }),
        })
    }

    pub(crate) fn try_acquire(&self, operation: &str) -> Option<OperationLease> {
        let mut active = self.inner.active.lock();
        if active.len() >= 256 || !active.insert(operation.into()) {
            return None;
        }
        Some(OperationLease {
            inner: self.inner.clone(),
            operation: operation.into(),
        })
    }

    pub(super) fn lease_active(&self, key: &str) -> bool {
        self.inner.active.lock().contains(key)
    }

    pub fn monitor_in_flight(&self, operation: &str) -> bool {
        self.lease_active(&format!("monitor:{operation}"))
    }

    pub fn admit_run(&self, scope: &RunScope, unit: &str, ceiling: u64) -> Result<(), String> {
        if !token(unit) || ceiling > MAX_COST_AMOUNT {
            return Err("invalid spending authorization".into());
        }
        let key = serde_json::to_string(scope).map_err(|_| "invalid scope")?;
        let db = self.inner.db.lock();
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
        let mut db = self.inner.db.lock();
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
            question_revision: 0,
            plugin: plugin.into(),
            request: request.clone(),
            state: OperationState::Prepared,
            job: None,
            waiting: None,
            estimate: None,
            reserved: 0,
            authorized: None,
            cancel_requested: false,
            cancel_dispatched: false,
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
        read(&self.inner.db.lock(), operation)
    }

    pub fn request_cancel(
        &self,
        operation: &str,
        scope: &RunScope,
        owner: &str,
    ) -> Result<OperationRecord, String> {
        self.change(operation, |record| {
            if &record.request.context.scope != scope || record.request.context.owner != owner {
                return Err("plugin cancellation scope refused".into());
            }
            if !record.state.terminal() {
                record.cancel_requested = true;
                record.state = OperationState::Unknown;
            }
            Ok(())
        })?;
        self.get(operation)?
            .ok_or("plugin operation unavailable".into())
    }

    pub fn mark_uncertain(
        &self,
        operation: &str,
        scope: &RunScope,
        owner: &str,
    ) -> Result<(), String> {
        self.change(operation, |record| {
            if &record.request.context.scope != scope || record.request.context.owner != owner {
                return Err("plugin operation scope refused".into());
            }
            if !record.state.terminal() {
                record.state = OperationState::Unknown;
            }
            Ok(())
        })
    }

    pub fn recover_inflight(&self) -> Result<(), String> {
        let mut db = self.inner.db.lock();
        if !self.inner.active.lock().is_empty() {
            return Err("plugin operations are active".into());
        }
        let tx = db.transaction().map_err(db_error)?;
        tx.execute("UPDATE blueprint_plugin_operations SET record=json_set(record,'$.state','unknown') WHERE json_extract(record,'$.state')='submitting'", []).map_err(db_error)?;
        tx.execute("UPDATE blueprint_plugin_monitors SET record=json_set(record,'$.state','unknown') WHERE stopped=0 AND json_extract(record,'$.claim') IS NOT NULL", []).map_err(db_error)?;
        tx.commit().map_err(db_error)
    }

    pub fn advance_owner_question(
        &self,
        operation: &str,
        scope: &RunScope,
        owner: &str,
    ) -> Result<(), String> {
        self.change(operation, |record| {
            if &record.request.context.scope != scope
                || record.request.context.owner != owner
                || record.question_revision >= 4096
            {
                return Err("owner question scope or limit refused".into());
            }
            record.question_revision += 1;
            Ok(())
        })
    }

    pub(crate) fn run_unit(&self, scope: &RunScope, unit: &str) -> Result<String, String> {
        let key = serde_json::to_string(scope).map_err(|_| "invalid scope")?;
        self.inner
            .db
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
        let db = self.inner.db.lock();
        let mut query = db.prepare("SELECT record FROM blueprint_plugin_operations WHERE scope=?1 ORDER BY rowid LIMIT 64").map_err(db_error)?;
        let rows = query
            .query_map([key], |r| r.get::<_, String>(0))
            .map_err(db_error)?;
        rows.map(|row| decode(&row.map_err(db_error)?)).collect()
    }

    pub(crate) fn reserve(
        &self,
        operation: &str,
        estimate: Option<Estimate>,
        now_ms: u64,
    ) -> Result<OperationRecord, String> {
        self.price(operation, estimate, now_ms, false)
    }

    pub(crate) fn refresh_price(
        &self,
        operation: &str,
        estimate: Option<Estimate>,
        now_ms: u64,
    ) -> Result<OperationRecord, String> {
        self.price(operation, estimate, now_ms, true)
    }

    pub fn authorize_fresh(
        &self,
        operation: &str,
        scope: &RunScope,
        actor: &str,
        maximum: u64,
        now_ms: u64,
    ) -> Result<(), String> {
        self.authorize(operation, scope, actor, maximum, now_ms, None)
    }

    pub fn authorize_quoted(
        &self,
        operation: &str,
        scope: &RunScope,
        actor: &str,
        maximum: u64,
        now_ms: u64,
        decision_key: &str,
    ) -> Result<(), String> {
        self.authorize(operation, scope, actor, maximum, now_ms, Some(decision_key))
    }

    fn authorize(
        &self,
        operation: &str,
        scope: &RunScope,
        actor: &str,
        maximum: u64,
        now_ms: u64,
        decision_key: Option<&str>,
    ) -> Result<(), String> {
        if maximum > MAX_COST_AMOUNT {
            return Err("allowance exceeds ledger bounds".into());
        }
        self.change(operation, |record| {
            if record.state != OperationState::WaitingForOwnerOk
                || decision_key.is_some_and(|key| owner_decision_key(record) != key)
                || &record.request.context.scope != scope
                || record.request.context.owner != actor
                || !record.estimate.as_ref().is_some_and(|estimate| {
                    estimate.unit == record.request.unit
                        && estimate.amount <= MAX_COST_AMOUNT
                        && estimate.valid_until_ms > now_ms
                        && estimate.valid_until_ms <= MAX_COST_AMOUNT
                })
            {
                return Err("fresh bounded owner authorization refused".into());
            }
            record.authorized = Some(OperationApproval {
                scope: scope.clone(),
                operation: operation.into(),
                actor: actor.into(),
                unit: record.request.unit.clone(),
                maximum,
                run_ceiling_exception: true,
            });
            Ok(())
        })
    }

    fn price(
        &self,
        operation: &str,
        estimate: Option<Estimate>,
        now_ms: u64,
        refresh_only: bool,
    ) -> Result<OperationRecord, String> {
        let mut db = self.inner.db.lock();
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
        if refresh_only {
            if record.question_revision >= 4096 {
                return Err("price refresh limit reached".into());
            }
            record.question_revision += 1;
            record.authorized = None;
        }
        let amount = estimate
            .as_ref()
            .filter(|estimate| estimate.unit == unit && estimate.amount <= MAX_COST_AMOUNT)
            .map(|estimate| estimate.amount);
        let reason = match estimate.as_ref() {
            None => Some(WaitingReason::EstimateMissing),
            Some(e)
                if e.unit != unit
                    || e.valid_until_ms <= now_ms
                    || e.valid_until_ms > MAX_COST_AMOUNT
                    || e.amount > MAX_COST_AMOUNT =>
            {
                Some(WaitingReason::EstimateStale)
            }
            Some(e)
                if record
                    .authorized
                    .as_ref()
                    .is_some_and(|approval| e.amount > approval.maximum) =>
            {
                Some(WaitingReason::OverStepAllowance)
            }
            Some(_) if record.authorized.is_some() => None,
            Some(e) if e.amount > record.request.step_allowance => {
                Some(WaitingReason::OverStepAllowance)
            }
            Some(e) if e.amount > remaining => Some(WaitingReason::OverRunCeiling),
            Some(_) if refresh_only => Some(WaitingReason::OwnerConfirmation),
            Some(_) => None,
        };
        record.estimate = estimate.filter(|estimate| {
            estimate.unit == unit
                && estimate.amount <= MAX_COST_AMOUNT
                && estimate.valid_until_ms <= MAX_COST_AMOUNT
        });
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
        let mut db = self.inner.db.lock();
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
