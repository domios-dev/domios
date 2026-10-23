use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, LazyLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::utility_deadline_model::{elapsed_seconds, parse_timeout, DeadlineModel};
use super::ServiceContext;

#[derive(Clone, Serialize, Deserialize)]
pub struct Deadline {
    tab_id: String,
    owner: String,
    #[serde(flatten)]
    model: DeadlineModel,
    command: Option<String>,
}

pub struct Armed {
    deadline: Deadline,
    cancel: Option<mpsc::Sender<()>>,
    generation: Arc<()>,
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

    pub fn set_utility_timeout(
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
            return Err("command timeout is stopping this shell's command".into());
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
        if tab.purpose == crate::multiplexer::types::ShellPurpose::Dev && duration.is_some() {
            return Err("dev servers do not support command timeouts".into());
        }
        if duration.is_none() {
            return remove_deadline(self, tab_id, &mut armed);
        }
        let mut model = DeadlineModel::default();
        model.set_timeout(duration)?;
        let current = COMMANDS.lock().get(tab_id).cloned();
        if let Some((_, started)) = &current {
            model.command_started(*started)?;
        }
        let deadline = Deadline {
            tab_id: tab_id.into(),
            owner: owner.into(),
            model,
            command: current.and_then(|(command, _)| command),
        };
        persist(self, &deadline)?;
        arm(self, deadline, &mut armed)?;
        Ok(())
    }

    pub(crate) fn cancel_utility_timeout(&self, tab_id: &str) -> Result<(), String> {
        let mut armed = ARMED.lock();
        remove_deadline(self, tab_id, &mut armed)
    }

    pub fn prepare_utility_command(&self, tab_id: &str, _command: &str) {
        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| tab.is_utility_shell()) {
                tab.utility_command_pending = true;
                tab.shell_command_state = Default::default();
            }
        }
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
        if EXPIRING.lock().contains(tab_id) {
            return;
        }
        let started = now_ms();
        COMMANDS
            .lock()
            .insert(tab_id.into(), (command.map(str::to_string), started));
        let Some(entry) = armed.get(tab_id) else {
            return;
        };
        let mut deadline = entry.deadline.clone();
        deadline.command = command.map(str::to_string);
        let result = deadline
            .model
            .command_started(started)
            .and_then(|()| arm(self, deadline.clone(), &mut armed))
            .and_then(|()| persist(self, &deadline));
        if let Err(error) = result {
            log::error!("[utility-timeout] command deadline failed: {error}");
        }
    }

    pub(crate) fn utility_command_finished(&self, tab_id: &str) -> Result<(), String> {
        let mut armed = ARMED.lock();
        COMMANDS.lock().remove(tab_id);
        let Some(entry) = armed.get(tab_id) else {
            return Ok(());
        };
        let mut deadline = entry.deadline.clone();
        deadline.model.command_finished();
        deadline.command = None;
        arm(self, deadline.clone(), &mut armed)?;
        persist(self, &deadline)
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
        let elapsed = deadline
            .model
            .started_ms
            .map(|started| elapsed_seconds(started, now_ms()));
        let result = self.restart_shell_tab(&deadline.tab_id, None, None);
        let message = format!(
            "Shell {} ({}) timed out; purpose: {}; running command: {}; command elapsed: {:.1}s; deadline_ms: {}; {}",
            tab.title, tab.id, tab.purpose.as_str(),
            deadline.command.as_deref().unwrap_or("unknown"),
            elapsed.unwrap_or_default(), deadline.model.deadline_ms.unwrap_or_default(),
            result.err().map(|error| format!("stop failed: {error}"))
                .unwrap_or_else(|| "command stopped; tab remains open".into())
        );
        self.utility_timeout_notice(&deadline.owner, &message);
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
    if let Some(cancel) = armed.remove(tab_id).and_then(|entry| entry.cancel) {
        let _ = cancel.send(());
    }
    Ok(())
}

fn arm(
    service: &ServiceContext,
    deadline: Deadline,
    armed: &mut HashMap<String, Armed>,
) -> Result<(), String> {
    let generation = Arc::new(());
    let cancel = if let Some(deadline_ms) = deadline.model.deadline_ms {
        let (cancel, cancelled) = mpsc::channel();
        let service = service.clone();
        let waiting = deadline.clone();
        let generation = generation.clone();
        std::thread::Builder::new()
            .name("utility-deadline".into())
            .spawn(move || {
                let remaining = Duration::from_millis(deadline_ms.saturating_sub(now_ms()));
                if cancelled.recv_timeout(remaining) != Err(mpsc::RecvTimeoutError::Timeout) {
                    return;
                }
                let armed = ARMED.lock();
                let Some(entry) = armed.get(&waiting.tab_id) else {
                    return;
                };
                if !Arc::ptr_eq(&entry.generation, &generation)
                    || !entry.deadline.model.expired(now_ms().max(deadline_ms))
                {
                    return;
                }
                EXPIRING.lock().insert(waiting.tab_id.clone());
                drop(armed);
                service.expire_utility_timeout(waiting.clone());
                EXPIRING.lock().remove(&waiting.tab_id);
            })
            .map_err(|e| e.to_string())?;
        Some(cancel)
    } else {
        None
    };
    if let Some(cancel) = armed
        .insert(
            deadline.tab_id.clone(),
            Armed {
                deadline,
                cancel,
                generation,
            },
        )
        .and_then(|entry| entry.cancel)
    {
        let _ = cancel.send(());
    }
    Ok(())
}

pub fn restore(service: &ServiceContext) {
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
            let tab = service.mux.read().get_tab(&deadline.tab_id).cloned();
            if tab.is_none() {
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
            if !tab.is_some_and(|tab| {
                tab.is_utility_shell()
                    && tab.purpose != crate::multiplexer::types::ShellPurpose::Dev
            }) {
                return remove_deadline(service, &deadline.tab_id, &mut ARMED.lock());
            }
            if deadline.model.started_ms.is_some() {
                service.utility_timeout_notice(
                    &deadline.owner,
                    &format!(
                        "Shell {} running command lost during daemon restart; command: {}",
                        deadline.tab_id,
                        deadline.command.as_deref().unwrap_or("unknown")
                    ),
                );
            }
            deadline.command = None;
            deadline.model.command_finished();
            persist(service, &deadline)?;
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
