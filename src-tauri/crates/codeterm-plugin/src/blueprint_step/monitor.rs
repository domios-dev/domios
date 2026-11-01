use super::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

const MAX_CHECKS: u32 = 4096;
const MAX_LIFETIME_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const CHECK_LEASE_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginJobScope {
    pub plugin: String,
    pub workspace: String,
    pub team: Option<String>,
    pub owner: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginJobOrigin {
    Blueprint {
        blueprint: String,
        run: String,
        node: String,
        attempt: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginJobRegistration {
    pub scope: PluginJobScope,
    pub origin: PluginJobOrigin,
    pub operation: String,
    pub job: String,
    pub next_check_ms: u64,
    pub deadline_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorStop {
    Deadline,
    CheckLimit,
    OwnerStopped,
    ScopeRemoved,
    PluginUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginJobMonitor {
    pub registration: PluginJobRegistration,
    pub next_check_ms: u64,
    pub state: OperationState,
    pub checks: u32,
    pub cancel_requested: bool,
    pub cancel_dispatched: bool,
    pub stopped: Option<MonitorStop>,
    claim: Option<String>,
}

pub struct MonitorClaim {
    pub monitor: PluginJobMonitor,
    pub cancel: bool,
    token: String,
    _leases: Vec<super::ledger::OperationLease>,
}

impl PluginJobMonitor {
    pub fn public_view(&self) -> Result<serde_json::Value, String> {
        let mut value = serde_json::to_value(self).map_err(|_| "invalid plugin monitor view")?;
        if let Some(object) = value.as_object_mut() {
            object.remove("claim");
        }
        Ok(value)
    }
}

impl OperationLedger {
    /// The caller must resolve this scope and origin from host authority before admission.
    pub fn monitor_existing_job(
        &self,
        registration: PluginJobRegistration,
        now_ms: u64,
    ) -> Result<PluginJobMonitor, String> {
        if !token(&registration.scope.plugin)
            || !valid_job_identity(&registration.job)
            || registration.operation.is_empty()
            || registration.operation.len() > 128
            || !registration.valid_identity()
            || serde_json::to_vec(&registration)
                .map_err(|_| "invalid job registration")?
                .len()
                > 4096
        {
            return Err("invalid bounded job monitor registration".into());
        }
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(monitor_error)?;
        if let Some(existing) = read_monitor(&tx, &registration.operation)? {
            if existing.registration != registration {
                return Err("job monitor identity changed".into());
            }
            return Ok(existing);
        }
        if registration.deadline_ms <= now_ms
            || registration.deadline_ms > now_ms.saturating_add(MAX_LIFETIME_MS)
            || registration.next_check_ms < now_ms.saturating_add(1000)
            || registration.next_check_ms > now_ms.saturating_add(900_000)
            || registration.next_check_ms > registration.deadline_ms
        {
            return Err("invalid bounded job monitor schedule".into());
        }
        let (total, plugin): (u32, u32) = tx.query_row("SELECT count(*), coalesce(sum(plugin=?1),0) FROM blueprint_plugin_monitors WHERE stopped=0", [&registration.scope.plugin], |row| Ok((row.get(0)?, row.get(1)?))).map_err(monitor_error)?;
        if total >= 1024 || plugin >= 256 {
            return Err("plugin monitor capacity reached".into());
        }
        let monitor = PluginJobMonitor {
            next_check_ms: registration.next_check_ms,
            registration,
            state: OperationState::Unknown,
            checks: 0,
            cancel_requested: false,
            cancel_dispatched: false,
            stopped: None,
            claim: None,
        };
        tx.execute("INSERT INTO blueprint_plugin_monitors(operation,plugin,due,deadline,stopped,claim_until,record) VALUES(?1,?2,?3,?4,0,0,?5)", params![monitor.registration.operation, monitor.registration.scope.plugin, monitor.registration.next_check_ms, monitor.registration.deadline_ms, encode_monitor(&monitor)?]).map_err(monitor_error)?;
        tx.commit().map_err(monitor_error)?;
        Ok(monitor)
    }

    pub fn job_monitor(&self, operation: &str) -> Result<Option<PluginJobMonitor>, String> {
        read_monitor(&self.inner.db.lock(), operation)
    }

    pub fn next_job_check(&self) -> Result<Option<u64>, String> {
        let full = (0..4).all(|slot| self.lease_active(&format!("monitor-worker:{slot}")));
        let db = self.inner.db.lock();
        let mut query = db.prepare("SELECT record,claim_until FROM blueprint_plugin_monitors WHERE stopped=0 LIMIT 1024").map_err(monitor_error)?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(monitor_error)?;
        let mut due = None;
        for row in rows {
            let (record, claimed_until) = row.map_err(monitor_error)?;
            let monitor: PluginJobMonitor =
                serde_json::from_str(&record).map_err(|_| "invalid stored plugin monitor")?;
            let busy = full
                || self.lease_active(&format!(
                    "monitor-plugin:{}",
                    monitor.registration.scope.plugin
                ));
            let next = if busy {
                monitor.registration.deadline_ms
            } else {
                monitor
                    .next_check_ms
                    .max(claimed_until)
                    .min(monitor.registration.deadline_ms)
            };
            due = Some(due.map_or(next, |previous: u64| previous.min(next)));
        }
        Ok(due)
    }

    pub fn active_job_monitors(&self) -> Result<Vec<PluginJobMonitor>, String> {
        let db = self.inner.db.lock();
        let mut query = db.prepare("SELECT record FROM blueprint_plugin_monitors WHERE stopped=0 ORDER BY operation LIMIT 1024").map_err(monitor_error)?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(monitor_error)?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(monitor_error)?)
                .map_err(|_| "invalid stored plugin monitor".into())
        })
        .collect()
    }

    pub fn claim_job_check(&self, now_ms: u64) -> Result<Option<MonitorClaim>, String> {
        for monitor in self.active_job_monitors()? {
            let reason = if now_ms >= monitor.registration.deadline_ms {
                Some(MonitorStop::Deadline)
            } else if monitor.checks >= MAX_CHECKS && monitor.claim.is_none() {
                Some(MonitorStop::CheckLimit)
            } else {
                None
            };
            if let Some(reason) = reason {
                self.stop_job_monitor(
                    &monitor.registration.operation,
                    &monitor.registration.scope,
                    reason,
                )?;
            }
        }
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(monitor_error)?;
        let active: u32 = tx
            .query_row(
                "SELECT count(*) FROM blueprint_plugin_monitors WHERE stopped=0 AND claim_until>?1",
                [now_ms],
                |row| row.get(0),
            )
            .map_err(monitor_error)?;
        if active >= 4 {
            return Ok(None);
        }
        let mut query = tx.prepare("SELECT record FROM blueprint_plugin_monitors m WHERE stopped=0 AND due<=?1 AND claim_until<=?1 AND NOT EXISTS(SELECT 1 FROM blueprint_plugin_monitors a WHERE a.plugin=m.plugin AND a.claim_until>?1 AND a.stopped=0) ORDER BY due,operation LIMIT 16").map_err(monitor_error)?;
        let candidates = query
            .query_map([now_ms], |row| row.get::<_, String>(0))
            .map_err(monitor_error)?
            .map(|row| {
                row.map_err(monitor_error).and_then(|value| {
                    serde_json::from_str::<PluginJobMonitor>(&value)
                        .map_err(|_| "invalid stored plugin monitor".into())
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        drop(query);
        for mut monitor in candidates {
            if monitor.registration.deadline_ms <= now_ms || monitor.checks >= MAX_CHECKS {
                monitor.stopped = Some(if monitor.checks >= MAX_CHECKS {
                    MonitorStop::CheckLimit
                } else {
                    MonitorStop::Deadline
                });
                monitor.claim = None;
                write_monitor(&tx, &monitor, 0)?;
                continue;
            }
            let Some(lease) =
                self.try_acquire(&format!("monitor:{}", monitor.registration.operation))
            else {
                continue;
            };
            let Some(plugin_lease) = self.try_acquire(&format!(
                "monitor-plugin:{}",
                monitor.registration.scope.plugin
            )) else {
                continue;
            };
            let Some(slot_lease) =
                (0..4).find_map(|slot| self.try_acquire(&format!("monitor-worker:{slot}")))
            else {
                continue;
            };
            let token = uuid::Uuid::new_v4().simple().to_string();
            if monitor.claim.is_some() {
                monitor.state = OperationState::Unknown;
            }
            monitor.claim = Some(token.clone());
            monitor.checks += 1;
            let cancel = monitor.cancel_requested && !monitor.cancel_dispatched;
            monitor.cancel_dispatched |= cancel;
            write_monitor(&tx, &monitor, now_ms.saturating_add(CHECK_LEASE_MS))?;
            tx.commit().map_err(monitor_error)?;
            return Ok(Some(MonitorClaim {
                monitor,
                cancel,
                token,
                _leases: vec![lease, plugin_lease, slot_lease],
            }));
        }
        tx.commit().map_err(monitor_error)?;
        Ok(None)
    }

    /// A callback observes only this exact job; a replacement identity is refused.
    pub fn settle_job_check(
        &self,
        claim: MonitorClaim,
        job: &str,
        state: OperationState,
        now_ms: u64,
        interval_ms: u64,
    ) -> Result<PluginJobMonitor, String> {
        if job != claim.monitor.registration.job
            || !(1000..=900_000).contains(&interval_ms)
            || !matches!(
                state,
                OperationState::Queued
                    | OperationState::Running
                    | OperationState::Succeeded
                    | OperationState::Failed
                    | OperationState::Canceled
                    | OperationState::Unknown
            )
        {
            return Err("invalid exact-job reconciliation result".into());
        }
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(monitor_error)?;
        let mut current = read_monitor(&tx, &claim.monitor.registration.operation)?
            .ok_or("plugin monitor missing")?;
        if current.claim.as_deref() != Some(&claim.token) || current.stopped.is_some() {
            return Err("plugin monitor lease revoked".into());
        }
        current.state = if current.cancel_requested && !state.terminal() {
            OperationState::Unknown
        } else {
            state
        };
        current.next_check_ms = now_ms
            .saturating_add(if current.cancel_requested && !current.cancel_dispatched {
                1000
            } else {
                interval_ms
            })
            .min(current.registration.deadline_ms);
        current.claim = None;
        write_monitor(&tx, &current, 0)?;
        tx.commit().map_err(monitor_error)?;
        Ok(current)
    }

    pub fn stop_job_monitor(
        &self,
        operation: &str,
        scope: &PluginJobScope,
        reason: MonitorStop,
    ) -> Result<(), String> {
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(monitor_error)?;
        let mut monitor = read_monitor(&tx, operation)?.ok_or("plugin monitor missing")?;
        if &monitor.registration.scope != scope {
            return Err("plugin monitor scope refused".into());
        }
        monitor.stopped = Some(reason);
        monitor.claim = None;
        write_monitor(&tx, &monitor, 0)?;
        tx.commit().map_err(monitor_error)
    }

    pub fn request_job_cancel(
        &self,
        operation: &str,
        scope: &PluginJobScope,
    ) -> Result<(), String> {
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(monitor_error)?;
        let mut monitor = read_monitor(&tx, operation)?.ok_or("plugin monitor missing")?;
        if &monitor.registration.scope != scope {
            return Err("plugin monitor scope refused".into());
        }
        if !monitor.state.terminal() {
            monitor.cancel_requested = true;
            monitor.state = OperationState::Unknown;
        }
        tx.execute(
            "UPDATE blueprint_plugin_monitors SET record=?2 WHERE operation=?1",
            params![operation, encode_monitor(&monitor)?],
        )
        .map_err(monitor_error)?;
        tx.commit().map_err(monitor_error)
    }

    pub fn wake_job_monitor(
        &self,
        operation: &str,
        scope: &PluginJobScope,
        now: u64,
    ) -> Result<(), String> {
        let mut db = self.inner.db.lock();
        let tx = db.transaction().map_err(monitor_error)?;
        let mut monitor = read_monitor(&tx, operation)?.ok_or("plugin monitor missing")?;
        if &monitor.registration.scope != scope
            || monitor.state.terminal()
            || monitor.checks >= MAX_CHECKS
            || monitor.registration.deadline_ms <= now
        {
            return Err("plugin monitor scope or lifetime exhausted".into());
        }
        monitor.stopped = None;
        monitor.next_check_ms = now
            .saturating_add(1000)
            .min(monitor.registration.deadline_ms);
        tx.execute(
            "UPDATE blueprint_plugin_monitors SET stopped=0,due=?2,record=?3 WHERE operation=?1",
            params![operation, monitor.next_check_ms, encode_monitor(&monitor)?],
        )
        .map_err(monitor_error)?;
        tx.commit().map_err(monitor_error)
    }
}

fn monitor_error(_: rusqlite::Error) -> String {
    "plugin monitor persistence failed".into()
}
fn encode_monitor(monitor: &PluginJobMonitor) -> Result<String, String> {
    serde_json::to_string(monitor).map_err(|_| "invalid plugin monitor".into())
}
fn read_monitor(
    db: &rusqlite::Connection,
    operation: &str,
) -> Result<Option<PluginJobMonitor>, String> {
    db.query_row(
        "SELECT record FROM blueprint_plugin_monitors WHERE operation=?1",
        [operation],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(monitor_error)?
    .map(|value| serde_json::from_str(&value).map_err(|_| "invalid stored plugin monitor".into()))
    .transpose()
}
fn write_monitor(
    db: &rusqlite::Connection,
    monitor: &PluginJobMonitor,
    claim_until: u64,
) -> Result<(), String> {
    db.execute("UPDATE blueprint_plugin_monitors SET due=?2,stopped=?3,claim_until=?4,record=?5 WHERE operation=?1", params![monitor.registration.operation, monitor.next_check_ms, monitor.stopped.is_some() || monitor.state.terminal(), claim_until, encode_monitor(monitor)?]).map_err(monitor_error)?;
    Ok(())
}

impl PluginJobRegistration {
    pub(super) fn valid_identity(&self) -> bool {
        let PluginJobOrigin::Blueprint {
            blueprint,
            run,
            node,
            attempt,
        } = &self.origin;
        [blueprint, run, node, attempt]
            .into_iter()
            .chain([&self.scope.workspace, &self.scope.owner, &self.operation])
            .chain(self.scope.team.iter())
            .all(|value| {
                !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
            })
    }
}
