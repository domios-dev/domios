use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, LazyLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::utility_deadline_model::{
    deadline_from_start, elapsed_seconds, parse_timeout, DeadlineAction, DeadlineModel,
};
use super::ServiceContext;

#[derive(Clone, Serialize, Deserialize)]
pub struct Deadline {
    tab_id: String,
    owner: String,
    started_ms: u64,
    deadline_ms: u64,
    command: Option<String>,
    #[serde(default)]
    command_started_ms: Option<u64>,
}

pub struct Armed {
    deadline: Deadline,
    cancel: mpsc::Sender<()>,
}

static ARMED: LazyLock<Mutex<HashMap<String, Armed>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

type RunningCommands = HashMap<String, (Option<String>, u64)>;
static COMMANDS: LazyLock<Mutex<RunningCommands>> = LazyLock::new(|| Mutex::new(HashMap::new()));

static EXPIRING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn path(service: &ServiceContext, tab_id: &str) -> std::path::PathBuf {
    service
        .agent_pad_data_dir
        .join("utility-deadlines")
        .join(format!("{tab_id}.json"))
}

impl ServiceContext {
    pub(crate) fn utility_deadline_snapshot(&self, tab_id: &str) -> Option<serde_json::Value> {
        ARMED
            .lock()
            .get(tab_id)
            .map(|entry| serde_json::to_value(&entry.deadline).unwrap_or_default())
    }

    pub(crate) fn set_utility_timeout(
        &self,
        tab_id: &str,
        owner: &str,
        value: &str,
    ) -> Result<(), String> {
        let duration = parse_timeout(value)?;
        let tab = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Shell not found")?;
        if !tab.is_utility_shell() {
            return Err("timeout requires a utility shell".into());
        }
        let mut armed = ARMED.lock();
        if EXPIRING.lock().contains(tab_id) {
            return Err("shell deadline already expired".into());
        }
        let previous = armed.get(tab_id);
        let owned = previous.map(|entry| entry.deadline.owner.as_str()).or(tab
            .close_restore
            .as_ref()
            .and_then(|state| state.utility_owner_tab_id.as_deref()));
        if owned != Some(owner) {
            return Err("only the owning agent can set this shell's timeout".into());
        }
        if !self
            .mux
            .read()
            .get_tab(owner)
            .is_some_and(|tab| tab.agent_type.is_some())
        {
            return Err("timeout requires a live owning agent".into());
        }
        let Some(duration) = duration else {
            remove_deadline(self, tab_id, &mut armed)?;
            return Ok(());
        };
        let started_ms = now_ms();
        let current = COMMANDS.lock().get(tab_id).cloned();
        let deadline = Deadline {
            tab_id: tab_id.into(),
            owner: owner.into(),
            started_ms,
            deadline_ms: deadline_from_start(started_ms, duration)?,
            command_started_ms: current
                .as_ref()
                .map(|(_, started)| *started)
                .or_else(|| previous.and_then(|entry| entry.deadline.command_started_ms)),
            command: current.map(|(command, _)| command).unwrap_or_else(|| {
                previous
                    .and_then(|entry| entry.deadline.command.clone())
                    .or_else(|| {
                        tab.close_restore
                            .as_ref()
                            .and_then(|state| state.command.clone())
                    })
            }),
        };
        persist(self, &deadline)?;
        arm(self, deadline, &mut armed)?;
        Ok(())
    }

    pub(crate) fn cancel_utility_timeout(&self, tab_id: &str) -> Result<(), String> {
        let mut armed = ARMED.lock();
        if EXPIRING.lock().contains(tab_id) {
            return Ok(());
        }
        remove_deadline(self, tab_id, &mut armed)
    }

    pub(crate) fn prepare_utility_command(&self, tab_id: &str, command: &str) {
        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| tab.is_utility_shell()) {
                tab.utility_command_pending = true;
                tab.shell_command_state = Default::default();
            }
        }
        self.utility_command_started(tab_id, Some(command));
    }

    pub(crate) fn utility_command_started(&self, tab_id: &str, command: Option<&str>) {
        if !self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.is_utility_shell())
        {
            return;
        }
        let mut armed = ARMED.lock();
        let started = now_ms();
        COMMANDS
            .lock()
            .insert(tab_id.into(), (command.map(str::to_string), started));
        if let Some(entry) = armed.get_mut(tab_id) {
            entry.deadline.command = command.map(str::to_string);
            entry.deadline.command_started_ms = Some(started);
        }
    }

    pub(crate) fn forget_utility_command(&self, tab_id: &str) {
        COMMANDS.lock().remove(tab_id);
    }

    fn expire_utility_timeout(&self, deadline: Deadline) {
        let tab = self.mux.read().get_tab(&deadline.tab_id).cloned();
        let Some(tab) = tab else {
            let _ = self.cancel_utility_timeout(&deadline.tab_id);
            self.forget_utility_command(&deadline.tab_id);
            return;
        };
        let activity = if tab.restored_from_snapshot
            && tab.shell_command_state.activity == crate::multiplexer::types::ShellActivity::Unknown
        {
            crate::multiplexer::types::ShellActivity::Unknown
        } else if tab.pty_id.is_none() && !tab.utility_command_pending {
            crate::multiplexer::types::ShellActivity::Idle
        } else if tab.utility_command_pending {
            crate::multiplexer::types::ShellActivity::Unknown
        } else {
            tab.shell_command_state.activity
        };
        let mut model = DeadlineModel::default();
        model.reset(Some(deadline.deadline_ms));
        let action = model.expire(now_ms().max(deadline.deadline_ms), activity);
        let result = self.close_tab_with_reason(
            &deadline.tab_id,
            crate::multiplexer::types::TabCloseReason::UtilityTimeout,
        );
        if action == Some(DeadlineAction::TerminateAndNotice) || result.is_err() {
            let command = deadline.command;
            let message = format!(
                "Shell {} ({}) timed out; purpose: {}; running command: {}; shell lifetime elapsed: {:.1}s; command elapsed: {}; started_ms: {}; deadline_ms: {}{}",
                tab.title,
                tab.id,
                tab.purpose.as_str(),
                command.as_deref().unwrap_or("unknown"),
                elapsed_seconds(deadline.started_ms, now_ms()),
                deadline.command_started_ms
                    .map(|started| format!("{:.1}s", elapsed_seconds(started, now_ms())))
                    .unwrap_or_else(|| "unknown".into()),
                deadline.started_ms,
                deadline.deadline_ms,
                result
                    .err()
                    .map(|error| format!("; close failed: {error}"))
                    .unwrap_or_default()
            );
            self.utility_timeout_notice(&deadline.owner, &message);
        }
    }

    fn utility_timeout_notice(&self, owner: &str, message: &str) {
        if let Err(error) = self.gateway_deliver(
            owner,
            super::messenger::MessageEnvelope::from_codeterm(),
            message,
            super::message_gateway::DeliveryClass::SystemNotice,
        ) {
            self.log_system_notice(
                "Utility shell timeout delivery failed".into(),
                Some(format!("{message}; {error}")),
                Some("warning"),
            );
        }
    }
}

fn persist(service: &ServiceContext, deadline: &Deadline) -> Result<(), String> {
    let path = path(service, &deadline.tab_id);
    std::fs::create_dir_all(path.parent().ok_or("deadline directory missing")?)
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(deadline).map_err(|e| e.to_string())?;
    codeterm_util::atomic_write::write_atomic(&path, &bytes).map_err(|e| e.to_string())
}

fn remove_deadline(
    service: &ServiceContext,
    tab_id: &str,
    armed: &mut HashMap<String, Armed>,
) -> Result<(), String> {
    match std::fs::remove_file(path(service, tab_id)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if let Some(previous) = armed.remove(tab_id) {
        let _ = previous.cancel.send(());
    }
    Ok(())
}

pub fn arm(
    service: &ServiceContext,
    deadline: Deadline,
    armed: &mut HashMap<String, Armed>,
) -> Result<(), String> {
    let (cancel, cancelled) = mpsc::channel();
    let service = service.clone();
    let waiting = deadline.clone();
    std::thread::Builder::new()
        .name("utility-deadline".into())
        .spawn(move || {
            let remaining = Duration::from_millis(waiting.deadline_ms.saturating_sub(now_ms()));
            if cancelled.recv_timeout(remaining) != Err(mpsc::RecvTimeoutError::Timeout) {
                return;
            }
            // Serialize expiry with resets so an expired generation cannot close an extended shell.
            let mut armed = ARMED.lock();
            let Some(entry) = armed.get(&waiting.tab_id) else {
                return;
            };
            if entry.deadline.deadline_ms != waiting.deadline_ms {
                return;
            }
            let expired = entry.deadline.clone();
            EXPIRING.lock().insert(waiting.tab_id.clone());
            armed.remove(&waiting.tab_id);
            drop(armed);
            service.expire_utility_timeout(expired);
            let mut armed = ARMED.lock();
            if let Err(error) = remove_deadline(&service, &waiting.tab_id, &mut armed) {
                log::error!("[utility-timeout] expired deadline cleanup failed: {error}");
            }
            EXPIRING.lock().remove(&waiting.tab_id);
        })
        .map_err(|e| e.to_string())?;
    if let Some(previous) = armed.insert(deadline.tab_id.clone(), Armed { deadline, cancel }) {
        let _ = previous.cancel.send(());
    }
    Ok(())
}

pub(crate) fn restore(service: &ServiceContext) {
    let directory = service.agent_pad_data_dir.join("utility-deadlines");
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            service.log_system_notice(
                "Utility deadlines lost".into(),
                Some(error.to_string()),
                Some("warning"),
            );
            return;
        }
    };
    for entry in entries {
        let result = entry.map_err(|e| e.to_string()).and_then(|entry| {
            if entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "json")
            {
                return Ok(());
            }
            let bytes = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
            let mut deadline: Deadline =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if service.mux.read().get_tab(&deadline.tab_id).is_none() {
                service.utility_timeout_notice(
                    &deadline.owner,
                    &format!(
                        "Shell {} timeout lost during daemon restart; running command: {}",
                        deadline.tab_id,
                        deadline.command.as_deref().unwrap_or("unknown")
                    ),
                );
                return remove_deadline(service, &deadline.tab_id, &mut ARMED.lock());
            }
            deadline.command = None;
            deadline.command_started_ms = None;
            arm(service, deadline, &mut ARMED.lock())
        });
        if let Err(error) = result {
            service.log_system_notice(
                "Utility deadline restore failed".into(),
                Some(error),
                Some("warning"),
            );
        }
    }
}

#[cfg(test)]
mod tests;
