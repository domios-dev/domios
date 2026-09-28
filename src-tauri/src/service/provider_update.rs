use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
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

pub fn check_available(service: Arc<ServiceContext>, api_port: u16, allow_non_prod: bool) {
    let providers = service.agent_registry.all_provider_arcs();
    let instance_override = std::env::var("CODETERM_INSTANCE").ok();
    let instance = offer_instance_name(instance_override.as_deref(), api_port, allow_non_prod);
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
                if let Some(instance) = instance.as_deref() {
                    offer(service.as_ref(), provider, version, command, instance);
                } else {
                    log::info!(
                        "[provider-update] {} update available: {} -> {}; this instance cannot offer provider updates",
                        provider.display_name(), version.current_version, version.latest_version
                    );
                }
            }
        });
    if let Err(error) = spawned {
        log::error!("[provider-update] version-check utility thread failed: {error}");
    }
}

fn offer_instance_name(
    instance_override: Option<&str>,
    api_port: u16,
    allow_non_prod: bool,
) -> Option<String> {
    if let Some(name) = instance_override {
        let name = name.trim();
        if !name.is_empty() {
            return allow_non_prod.then(|| name.to_string());
        }
    }
    if api_port == 7685 {
        return Some("production".into());
    }
    allow_non_prod.then(|| format!("instance on port {api_port}"))
}

fn offer(
    service: &ServiceContext,
    provider: Arc<dyn AgentProvider>,
    version: ProviderUpdateAvailability,
    command: String,
    instance: &str,
) {
    let key = format!("{}:{}", provider.id(), version.latest_version);
    if OFFERED_VERSIONS.lock().contains(&key) {
        return;
    }
    let Ok(_lock) = acquire_machine_update_lock() else {
        log::info!("[provider-update] offer deferred because another instance holds the machine update lock");
        return;
    };
    if version_was_offered(&key) {
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
        format!("{} update available in {instance}", provider.display_name()),
        Some(format!(
            "{instance}: {} {} -> {} is available.",
            provider.display_name(),
            version.current_version,
            version.latest_version
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
    OFFERED_VERSIONS.lock().insert(key.clone());
    if let Err(error) = record_version_offered(&key) {
        log::warn!("[provider-update] could not persist offer deduplication: {error}");
    }
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

pub(super) fn is_offer(id: &str) -> bool {
    OFFERS.lock().contains_key(id)
}

pub fn on_state_change(service: &ServiceContext) {
    process_accepted(service);
}

fn process_accepted(service: &ServiceContext) {
    let jobs = std::mem::take(&mut *ACCEPTED.lock());
    for (id, offer) in jobs {
        match restart_targets(service, &offer.provider) {
            Ok(RestartReadiness::Ready(_)) => start_update(service.clone(), id, offer),
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

fn newly_discovered_targets(
    targets: Vec<RestartTarget>,
    paused_ids: &HashSet<String>,
) -> Vec<RestartTarget> {
    targets
        .into_iter()
        .filter(|target| !paused_ids.contains(&target.tab_id))
        .collect()
}

enum RestartReadiness {
    Busy,
    SnapshotBusy,
    Ready(Vec<RestartTarget>),
}

#[derive(Debug, PartialEq, Eq)]
enum UpdateGate {
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

fn update_gate(statuses: &[Option<TabStatus>]) -> UpdateGate {
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
    if update_gate(&statuses) == UpdateGate::WaitForIdle {
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
        targets.push(RestartTarget {
            tab_id: tab_id.clone(),
            cwd: cwd.to_string(),
            session_id: session_id.to_string(),
        });
    }
    Ok(RestartReadiness::Ready(targets))
}

fn start_update(service: ServiceContext, id: String, offer: Offer) {
    if !INSTALLING.lock().insert(offer.provider.id().to_string()) {
        ACCEPTED.lock().insert(id, offer);
        return;
    }
    let provider_id = offer.provider.id().to_string();
    let spawned = std::thread::Builder::new()
        .name("provider-update-install".into())
        .spawn(move || {
            let outcome = acquire_machine_update_lock().and_then(|_lock| {
                let mut paused = Vec::new();
                let mut paused_ids = HashSet::new();
                loop {
                    let targets = match restart_targets(&service, &offer.provider)? {
                        RestartReadiness::Ready(targets) => targets,
                        RestartReadiness::Busy => {
                            let failures = resume_paused(&service, paused);
                            return Err(format!(
                                "a provider agent became busy before update; {}",
                                failures.join("; ")
                            ));
                        }
                        RestartReadiness::SnapshotBusy => {
                            let failures = resume_paused(&service, paused);
                            return Err(format!(
                                "provider state changed before update; {}",
                                failures.join("; ")
                            ));
                        }
                    };
                    let targets = newly_discovered_targets(targets, &paused_ids);
                    if targets.is_empty() {
                        break;
                    }
                    for target in targets {
                        if !offer
                            .provider
                            .session_exists(&target.cwd, &target.session_id)
                        {
                            let failures = resume_paused(&service, paused);
                            return Err(format!(
                                "Tab {} no longer has its bound session; {}",
                                target.tab_id,
                                failures.join("; ")
                            ));
                        }
                        match service.pause_for_provider_update(
                            &target.tab_id,
                            offer.provider.id(),
                            &target.session_id,
                        ) {
                            Ok(tab) => {
                                paused_ids.insert(target.tab_id);
                                paused.push(tab);
                            }
                            Err(error) => {
                                let resume_errors = resume_paused(&service, paused);
                                return Err(format!(
                                    "could not pause tabs: {error}; {}",
                                    resume_errors.join("; ")
                                ));
                            }
                        }
                    }
                }
                let update_result = run_update_command(&offer.command);
                if let Err(failure) = &update_result {
                    if !failure.child_stopped {
                        return Err(format!(
                            "{}; updater could not be stopped, so affected tabs remain stopped",
                            failure.reason
                        ));
                    }
                }
                let resume_errors = resume_paused(&service, paused);
                match (update_result, resume_errors.is_empty()) {
                    (Ok(()), true) => Ok(()),
                    (Err(error), _) => {
                        Err(format!("{}; {}", error.reason, resume_errors.join("; ")))
                    }
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

fn version_was_offered(key: &str) -> bool {
    let Some(path) = provider_update_state_dir().map(|dir| dir.join("provider-update-offers"))
    else {
        return true;
    };
    let mut contents = String::new();
    match File::open(&path) {
        Ok(mut file) if file.read_to_string(&mut contents).is_err() => return true,
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return true,
    }
    contents.lines().any(|line| line == key)
}

fn record_version_offered(key: &str) -> std::io::Result<()> {
    let path = provider_update_state_dir()
        .ok_or_else(|| std::io::Error::other("home directory is unavailable"))?
        .join("provider-update-offers");
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{key}")
}

fn acquire_machine_update_lock() -> Result<MachineUpdateLock, String> {
    let path = provider_update_state_dir()
        .ok_or_else(|| "could not resolve the home CodeTerm directory".to_string())?
        .join("provider-update.lock");
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
    file.set_len(0).map_err(|error| error.to_string())?;
    writeln!(&file, "{}", std::process::id()).map_err(|error| error.to_string())?;
    Ok(MachineUpdateLock(file))
}

fn provider_update_state_dir() -> Option<PathBuf> {
    let path = dirs::home_dir()?.join(".codeterm");
    std::fs::create_dir_all(&path).ok()?;
    Some(path)
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

struct UpdateCommandFailure {
    reason: String,
    child_stopped: bool,
}

fn run_update_command(command: &str) -> Result<(), UpdateCommandFailure> {
    use wait_timeout::ChildExt;
    let (executable, args) = update_argv(command).map_err(|reason| UpdateCommandFailure {
        reason,
        child_stopped: true,
    })?;
    let mut process = Command::new(executable);
    process.args(args);
    codeterm_util::process_util::no_window_std(&mut process);
    let mut child = process
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| UpdateCommandFailure {
            reason: error.to_string(),
            child_stopped: true,
        })?;
    let wait_result = match child.wait_timeout(std::time::Duration::from_secs(300)) {
        Ok(result) => result,
        Err(error) => {
            let killed = child.kill().is_ok();
            let stopped = if killed {
                child.wait().is_ok()
            } else {
                child.try_wait().ok().flatten().is_some()
            };
            return Err(UpdateCommandFailure {
                reason: format!("waiting for provider update failed: {error}"),
                child_stopped: stopped,
            });
        }
    };
    match wait_result {
        Some(status) if status.success() => Ok(()),
        Some(status) => Err(UpdateCommandFailure {
            reason: format!("provider update exited with {status}"),
            child_stopped: true,
        }),
        None => {
            let killed = child.kill().is_ok();
            let stopped = if killed {
                child.wait().is_ok()
            } else {
                child.try_wait().ok().flatten().is_some()
            };
            Err(UpdateCommandFailure {
                reason: "provider update exceeded the 300 second deadline".into(),
                child_stopped: stopped,
            })
        }
    }
}

fn update_argv(command: &str) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
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
    let resolved = match codeterm_plugin::js_runtime::probe_bin(executable).resolution {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
        codeterm_plugin::js_runtime::BinResolution::Missing
        | codeterm_plugin::js_runtime::BinResolution::Unknown => {
            #[cfg(windows)]
            return Err(format!(
                "provider update executable {executable} was not resolved through PATH/PATHEXT"
            ));
            #[cfg(not(windows))]
            PathBuf::from(executable)
        }
    };
    #[cfg(windows)]
    if resolved
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("cmd"))
    {
        return windows_npm_shim_argv(&resolved, &args);
    }
    Ok((resolved, args.into_iter().map(Into::into).collect()))
}

#[cfg(windows)]
fn windows_npm_shim_argv(
    shim_path: &Path,
    args: &[&str],
) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
    let contents = std::fs::read_to_string(shim_path).map_err(|error| error.to_string())?;
    let script = npm_shim_script(shim_path.parent().unwrap_or(Path::new(".")), &contents)
        .ok_or_else(|| {
            format!(
                "could not resolve the node entrypoint in {}",
                shim_path.display()
            )
        })?;
    let node = match codeterm_plugin::js_runtime::probe_bin("node").resolution {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
        _ => return Err("Node.js was not found while resolving the Codex Windows shim".into()),
    };
    let mut argv = vec![script.into_os_string()];
    argv.extend(args.iter().map(std::ffi::OsString::from));
    Ok((node, argv))
}

fn npm_shim_script(parent: &Path, contents: &str) -> Option<PathBuf> {
    let marker = "\"%_prog%\" \"";
    let line = contents.lines().find(|line| {
        line.to_ascii_lowercase()
            .contains(&marker.to_ascii_lowercase())
            && line.contains("%*")
    })?;
    let start = line
        .to_ascii_lowercase()
        .find(&marker.to_ascii_lowercase())?
        + marker.len();
    let target = line[start..].split_once('"')?.0;
    let relative = target
        .strip_prefix("%dp0%")?
        .trim_start_matches(['\\', '/']);
    let relative = relative.replace('\\', "/");
    Some(parent.join(relative))
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
    use super::{
        newly_discovered_targets, offer_instance_name, update_consent, update_gate,
        NotificationOrigin, PushIntent, RestartTarget, UpdateConsent, UpdateGate, LATER,
    };
    use codeterm_types::pty::TabStatus;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn provider_update_waits_for_owner_choice_and_idle_agents() {
        assert_eq!(update_consent(LATER), UpdateConsent::Declined);
        assert_eq!(
            update_gate(&[Some(TabStatus::Working)]),
            UpdateGate::WaitForIdle
        );
        assert_eq!(update_gate(&[Some(TabStatus::Idle)]), UpdateGate::Ready);
        assert_eq!(update_gate(&[None]), UpdateGate::WaitForIdle);
    }

    #[test]
    fn only_default_production_instance_offers_without_opt_in() {
        assert_eq!(
            offer_instance_name(None, 7685, false).as_deref(),
            Some("production")
        );
        assert_eq!(offer_instance_name(None, 7686, false), None);
        assert_eq!(offer_instance_name(Some("dev-a"), 7685, false), None);
        assert_eq!(
            offer_instance_name(Some("dev-a"), 7686, true).as_deref(),
            Some("dev-a")
        );
    }

    #[test]
    fn npm_windows_shim_resolves_to_a_direct_node_script() {
        let parent = Path::new("C:/Users/owner/AppData/Roaming/npm");
        let shim =
            "@ECHO off\n\"%_prog%\" \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\n";
        assert_eq!(
            super::npm_shim_script(parent, shim),
            Some(parent.join("node_modules/@openai/codex/bin/codex.js"))
        );
    }

    #[test]
    fn post_lock_snapshot_keeps_new_tabs_and_skips_already_paused_tabs() {
        let target = |tab_id: &str| RestartTarget {
            tab_id: tab_id.into(),
            cwd: "cwd".into(),
            session_id: format!("session-{tab_id}"),
        };
        let paused = HashSet::from(["already-paused".to_string()]);
        let remaining = newly_discovered_targets(
            vec![target("already-paused"), target("opened-after-consent")],
            &paused,
        );
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].tab_id, "opened-after-consent");
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn http_answer_is_refused_then_owner_answer_queues_update() {
        use super::{Offer, ACCEPTED, OFFERS, UPDATE_NOW};
        use crate::plugin::agent_claude::ClaudeProvider;
        use crate::service::test_fixture::ServiceFixture;
        use codeterm_plugin::provider::ProviderUpdateAvailability;
        use codeterm_types::notification::CallerAction;
        use codeterm_types::pty::{BootStage, TabVerdict, TimelineState};
        use std::sync::Arc;

        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        let tab_id = mux.create_tab(&workspace).unwrap();
        let pty_id = "update-consent-busy-pty";
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some(pty_id.into());
        tab.agent_type = Some("claude".into());
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.agent_state = Some(
            TabVerdict {
                state: TimelineState::Working,
                subtype: None,
            }
            .into(),
        );
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(ClaudeProvider::new()));
        fixture
            .service
            .agent_tracker
            .lock()
            .update_output(pty_id, [1u8; 32]);
        let id = fixture.service.create_notification_ext(
            "Update available".into(),
            Some("version 1 -> 2".into()),
            Some("info"),
            Some(vec![
                CallerAction::Answer {
                    label: UPDATE_NOW.into(),
                },
                CallerAction::Answer {
                    label: LATER.into(),
                },
            ]),
            Some(true),
            NotificationOrigin::default(),
            PushIntent {
                level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
                event_key: Some("provider-update-test".into()),
            },
            None,
        );
        OFFERS.lock().insert(
            id.into(),
            Offer {
                provider: Arc::new(ClaudeProvider::new()),
                version: ProviderUpdateAvailability {
                    current_version: "1.0.0".into(),
                    latest_version: "2.0.0".into(),
                },
                command: "codex update".into(),
            },
        );

        assert!(fixture
            .service
            .answer_notification(&id, UPDATE_NOW)
            .is_err());
        assert!(OFFERS.lock().contains_key(&id));
        fixture
            .service
            .answer_owner_notification(&id, UPDATE_NOW)
            .expect("owner answer should be recorded");
        assert!(ACCEPTED.lock().contains_key(&id));
        ACCEPTED.lock().remove(&id);
        OFFERS.lock().remove(&id);
    }
}
