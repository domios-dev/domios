use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use codeterm_plugin::provider::{AgentProvider, ProviderUpdateAvailability};
use codeterm_types::notification::CallerAction;
use codeterm_types::pty::TabStatus;
use parking_lot::Mutex;

use super::{
    notification::{NotificationOrigin, PushIntent},
    ServiceContext,
};
use crate::notification::answer::AnswerAdmission;

const UPDATE_NOW: &str = "Update now";
const LATER: &str = "Later";

struct Offer {
    provider: Arc<dyn AgentProvider>,
    version: ProviderUpdateAvailability,
    command: String,
}

static OFFERS: LazyLock<Mutex<HashMap<String, Offer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACCEPTED: LazyLock<Mutex<HashMap<String, Offer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static OFFERED_VERSIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static INSTALLING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static SNAPSHOT_WAKE_ARMED: AtomicBool = AtomicBool::new(false);

pub fn check_available(service: Arc<ServiceContext>) {
    let providers = service.agent_registry.all_provider_arcs();
    let spawned = std::thread::Builder::new()
        .name("provider-update-check".into())
        .spawn(move || {
            for provider in providers {
                if !provider.supports_update_check() {
                    continue;
                }
                let Some(command) = provider.update_command(platform_key()) else {
                    continue;
                };
                let Some(version) = provider.check_for_update() else {
                    continue;
                };
                offer(service.as_ref(), provider, version, command);
            }
        });
    if let Err(error) = spawned {
        log::error!("[provider-update] version-check utility thread failed: {error}");
    }
}

fn offer(
    service: &ServiceContext,
    provider: Arc<dyn AgentProvider>,
    version: ProviderUpdateAvailability,
    command: String,
) {
    let key = format!("{}:{}", provider.id(), version.latest_version);
    if !OFFERED_VERSIONS.lock().insert(key) {
        return;
    }
    let key = format!("{}:{}", provider.id(), version.latest_version);
    if !mark_version_offered(&key) {
        return;
    }
    let actions = Some(vec![
        CallerAction::Answer {
            label: UPDATE_NOW.to_string(),
        },
        CallerAction::Answer {
            label: LATER.to_string(),
        },
    ]);
    let id = service.create_notification_ext(
        format!("{} update available", provider.display_name()),
        Some(format!(
            "Version {} is installed. Version {} is available.",
            version.current_version, version.latest_version
        )),
        Some("info"),
        actions,
        Some(true),
        NotificationOrigin::default(),
        PushIntent {
            level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
            event_key: Some(format!("provider-update:{key}")),
        },
        None,
    );
    OFFERS.lock().insert(
        id,
        Offer {
            provider,
            version,
            command,
        },
    );
}

pub fn answer(
    service: &ServiceContext,
    id: &str,
    label: &str,
) -> Option<Result<serde_json::Value, String>> {
    let offer = OFFERS.lock().remove(id)?;
    let admission = service.notif.lock().record_detached_answer(id, label);
    let AnswerAdmission::Recorded(answer) = admission else {
        OFFERS.lock().insert(id.to_string(), offer);
        return Some(Err(
            "The provider update choice could not be recorded".into()
        ));
    };
    service.notif.lock().mark_answer_delivered(id);
    service.notify_event(
        "notification-answered",
        serde_json::json!({"id": id, "answered": answer, "delivered": true}),
    );
    if update_consent(label) == UpdateConsent::Accepted {
        ACCEPTED.lock().insert(id.to_string(), offer);
        process_accepted(service);
    }
    Some(Ok(
        serde_json::json!({ "answered": answer, "delivered": true }),
    ))
}

pub fn on_state_change(service: &ServiceContext) {
    process_accepted(service);
}

fn process_accepted(service: &ServiceContext) {
    let jobs = std::mem::take(&mut *ACCEPTED.lock());
    for (id, offer) in jobs {
        match restart_targets(service, &offer.provider) {
            Ok(RestartReadiness::Ready(targets)) => {
                start_update(service.clone(), id, offer, targets)
            }
            Ok(RestartReadiness::Busy) => {
                ACCEPTED.lock().insert(id, offer);
            }
            Ok(RestartReadiness::SnapshotBusy) => {
                ACCEPTED.lock().insert(id, offer);
                wake_after_snapshot(service);
            }
            Err(reason) => {
                log::warn!("[provider-update] update declined after consent: {reason}");
                notify_owner(
                    service,
                    format!("{} update is waiting", offer.provider.display_name()),
                    reason,
                );
            }
        }
    }
}

fn wake_after_snapshot(service: &ServiceContext) {
    if SNAPSHOT_WAKE_ARMED.swap(true, Ordering::AcqRel) {
        return;
    }
    let service = service.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("provider-update-state-wake".into())
        .spawn(move || {
            super::status_reconciliation::wait_until_snapshot_readable(
                &service.mux,
                &service.agent_tracker,
            );
            SNAPSHOT_WAKE_ARMED.store(false, Ordering::Release);
            process_accepted(&service);
        })
    {
        SNAPSHOT_WAKE_ARMED.store(false, Ordering::Release);
        log::error!("[provider-update] state wake thread failed: {error}");
    }
}

struct RestartTarget {
    tab_id: String,
    cwd: String,
    session_id: String,
}

enum RestartReadiness {
    Busy,
    SnapshotBusy,
    Ready(Vec<RestartTarget>),
}

#[derive(Debug, PartialEq, Eq)]
enum UpdateGate {
    NoConsent,
    WaitForIdle,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateConsent {
    Pending,
    Accepted,
    Declined,
}

fn update_consent(label: &str) -> UpdateConsent {
    match label {
        UPDATE_NOW => UpdateConsent::Accepted,
        LATER => UpdateConsent::Declined,
        _ => UpdateConsent::Pending,
    }
}

fn update_gate(consent: UpdateConsent, statuses: &[Option<TabStatus>]) -> UpdateGate {
    if consent != UpdateConsent::Accepted {
        return UpdateGate::NoConsent;
    }
    if statuses
        .iter()
        .any(|status| !matches!(status, Some(TabStatus::Idle | TabStatus::Dead)))
    {
        return UpdateGate::WaitForIdle;
    }
    UpdateGate::Ready
}

fn restart_targets(
    service: &ServiceContext,
    provider: &Arc<dyn AgentProvider>,
) -> Result<RestartReadiness, String> {
    let Some(snapshot) = service.try_snapshot_with_reconciled_status() else {
        return Ok(RestartReadiness::SnapshotBusy);
    };
    let tabs = snapshot
        .tabs
        .iter()
        .filter(|(_, tab)| tab.agent_type.as_deref() == Some(provider.id()))
        .collect::<Vec<_>>();
    let statuses = tabs
        .iter()
        .map(|(tab_id, _)| {
            snapshot.tab_statuses.get(*tab_id).copied().or_else(|| {
                (crate::agent::live_state::observe_tab_liveness(service, tab_id)
                    == codeterm_types::pty::AgentLiveness::Absent)
                    .then_some(TabStatus::Dead)
            })
        })
        .collect::<Vec<_>>();
    if update_gate(UpdateConsent::Accepted, &statuses) == UpdateGate::WaitForIdle {
        return Ok(RestartReadiness::Busy);
    }
    let mut targets = Vec::new();
    for ((tab_id, tab), status) in tabs.into_iter().zip(statuses) {
        match status {
            Some(TabStatus::Dead) => continue,
            Some(TabStatus::Idle) => {}
            Some(TabStatus::Working | TabStatus::Waiting) | None => unreachable!(),
        }
        let (Some(cwd), Some(session_id)) = (tab.cwd.as_deref(), tab.agent_session_id.as_deref())
        else {
            return Err(format!("Tab {tab_id} has no bound session to resume"));
        };
        if !provider.session_exists(cwd, session_id) {
            return Err(format!("Tab {tab_id} no longer has its bound session"));
        }
        targets.push(RestartTarget {
            tab_id: tab_id.clone(),
            cwd: cwd.to_string(),
            session_id: session_id.to_string(),
        });
    }
    Ok(RestartReadiness::Ready(targets))
}

fn start_update(service: ServiceContext, id: String, offer: Offer, targets: Vec<RestartTarget>) {
    if !INSTALLING.lock().insert(offer.provider.id().to_string()) {
        ACCEPTED.lock().insert(id, offer);
        return;
    }
    let provider_id = offer.provider.id().to_string();
    let spawned = std::thread::Builder::new()
        .name("provider-update-install".into())
        .spawn(move || {
            let outcome = acquire_machine_update_lock().and_then(|_lock| {
                match restart_targets(&service, &offer.provider)? {
                    RestartReadiness::Ready(_) => {}
                    RestartReadiness::Busy => {
                        return Err("a provider agent became busy before update".into())
                    }
                    RestartReadiness::SnapshotBusy => {
                        return Err("provider state changed before update".into())
                    }
                }
                let mut paused = Vec::new();
                for target in &targets {
                    match service.pause_for_provider_update(
                        &target.tab_id,
                        offer.provider.id(),
                        &target.session_id,
                    ) {
                        Ok(tab) => paused.push(tab),
                        Err(error) => {
                            let resume_errors = resume_paused(&service, paused);
                            return Err(format!(
                                "could not pause tabs: {error}; {}",
                                resume_errors.join("; ")
                            ));
                        }
                    }
                }
                let update_result = run_update_command(&offer.command);
                let resume_errors = resume_paused(&service, paused);
                match (update_result, resume_errors.is_empty()) {
                    (Ok(()), true) => Ok(()),
                    (Err(error), _) => Err(format!("{error}; {}", resume_errors.join("; "))),
                    (Ok(()), false) => {
                        Err(format!("tab resume failed: {}", resume_errors.join("; ")))
                    }
                }
            });
            match outcome {
                Ok(()) => {
                    service.create_notification(
                        format!("{} updated", offer.provider.display_name()),
                        Some(format!(
                            "Updated to version {}.",
                            offer.version.latest_version
                        )),
                        Some("info"),
                    );
                }
                Err(error) => {
                    log::error!("[provider-update] installer failed: {error}");
                    notify_owner(
                        &service,
                        format!("{} update failed", offer.provider.display_name()),
                        error,
                    );
                }
            }
            INSTALLING.lock().remove(&provider_id);
        });
    if let Err(error) = spawned {
        INSTALLING.lock().remove(&provider_id);
        log::error!("[provider-update] installer utility thread failed: {error}");
    }
}

fn resume_paused(
    service: &ServiceContext,
    paused: Vec<crate::service::agent_restart::ProviderUpdatePause>,
) -> Vec<String> {
    paused
        .into_iter()
        .filter_map(|tab| service.resume_after_provider_update(tab).err())
        .collect()
}

struct MachineUpdateLock(File);

fn mark_version_offered(key: &str) -> bool {
    let Ok(_lock) = acquire_machine_update_lock() else {
        return false;
    };
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let path = home.join(".codeterm-provider-update-offers");
    let mut contents = String::new();
    if let Ok(mut file) = File::open(&path) {
        if file.read_to_string(&mut contents).is_err() {
            return false;
        }
    }
    if contents.lines().any(|line| line == key) {
        return false;
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    writeln!(file, "{key}").is_ok()
}

fn acquire_machine_update_lock() -> Result<MachineUpdateLock, String> {
    let path = dirs::home_dir()
        .ok_or_else(|| "could not resolve the machine home directory".to_string())?
        .join(".codeterm-provider-update.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 };
    #[cfg(windows)]
    let locked = lock_windows_file(&file, true);
    if !locked {
        return Err(
            "another CodeTerm instance is already updating a provider on this machine".into(),
        );
    }
    Ok(MachineUpdateLock(file))
}

impl Drop for MachineUpdateLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
        #[cfg(windows)]
        lock_windows_file(&self.0, false);
    }
}

#[cfg(windows)]
fn lock_windows_file(file: &File, acquire: bool) -> bool {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut c_void,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(
            handle: *mut c_void,
            flags: u32,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            handle: *mut c_void,
            reserved: u32,
            low: u32,
            high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }

    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    unsafe {
        if acquire {
            LockFileEx(file.as_raw_handle().cast(), 0x3, 0, 1, 0, &mut overlapped) != 0
        } else {
            UnlockFileEx(file.as_raw_handle().cast(), 0, 1, 0, &mut overlapped) != 0
        }
    }
}

fn notify_owner(service: &ServiceContext, title: String, body: String) {
    service.create_notification_ext(
        title,
        Some(body),
        Some("warning"),
        None,
        Some(true),
        NotificationOrigin::default(),
        PushIntent {
            level: Some(crate::notification::push::NotifyLevel::Failure),
            event_key: None,
        },
        None,
    );
}

fn run_update_command(command: &str) -> Result<(), String> {
    use wait_timeout::ChildExt;
    let mut parts = command.split_whitespace();
    let executable = parts
        .next()
        .ok_or_else(|| "empty provider update command".to_string())?;
    if !executable
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_./\\:-".contains(ch))
    {
        return Err("provider update executable contains unsupported characters".into());
    }
    let args = parts.collect::<Vec<_>>();
    if args.iter().any(|arg| {
        !arg.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_./\\:-".contains(ch))
    }) {
        return Err("provider update arguments contain unsupported characters".into());
    }
    let mut process = Command::new(executable);
    process.args(args);
    codeterm_util::process_util::no_window_std(&mut process);
    let mut child = process
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    match child
        .wait_timeout(std::time::Duration::from_secs(300))
        .map_err(|error| error.to_string())?
    {
        Some(status) if status.success() => Ok(()),
        Some(status) => Err(format!("provider update exited with {status}")),
        None => {
            child
                .kill()
                .map_err(|error| format!("update timed out and could not be stopped: {error}"))?;
            let _ = child.wait();
            Err("provider update exceeded the 300 second deadline".into())
        }
    }
}

fn platform_key() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

#[cfg(test)]
mod tests {
    use super::{update_consent, update_gate, UpdateConsent, UpdateGate, LATER, UPDATE_NOW};
    use codeterm_types::pty::TabStatus;

    #[test]
    fn provider_update_waits_for_owner_choice_and_idle_agents() {
        assert_eq!(
            update_gate(update_consent(LATER), &[Some(TabStatus::Idle)]),
            UpdateGate::NoConsent
        );
        assert_eq!(
            update_gate(update_consent(UPDATE_NOW), &[Some(TabStatus::Working)]),
            UpdateGate::WaitForIdle
        );
        assert_eq!(
            update_gate(update_consent(UPDATE_NOW), &[Some(TabStatus::Idle)]),
            UpdateGate::Ready
        );
        assert_eq!(
            update_gate(update_consent(UPDATE_NOW), &[None]),
            UpdateGate::WaitForIdle
        );
        assert_eq!(update_consent(LATER), UpdateConsent::Declined);
    }
}
