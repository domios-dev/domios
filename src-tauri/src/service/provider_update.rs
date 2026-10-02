use std::collections::{HashMap, HashSet};
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

mod command;
mod storage;
mod state;
use command::{platform_key, run_update_command};
use storage::{acquire_machine_update_lock, version_was_offered, record_version_offered};
#[cfg(test)]
use command::npm_shim_script;

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
    if statuses.iter().any(|status| {
        !matches!(
            status,
            Some(TabStatus::Idle | TabStatus::Error | TabStatus::Dead)
        )
    }) {
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
            Some(TabStatus::Error) => {}
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
    let thread_provider_id = provider_id.clone();
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
            INSTALLING.lock().remove(&thread_provider_id);
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

#[cfg(test)]
mod tests;
