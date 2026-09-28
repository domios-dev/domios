use std::collections::{HashMap, HashSet};
use std::process::Command;
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
    if label == UPDATE_NOW {
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

fn update_gate(consented: bool, statuses: &[Option<TabStatus>]) -> UpdateGate {
    if !consented {
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
    if update_gate(true, &statuses) == UpdateGate::WaitForIdle {
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
            let outcome = run_update_command(&offer.command);
            match outcome {
                Ok(()) => {
                    let mut failures = Vec::new();
                    for target in targets {
                        if !offer
                            .provider
                            .session_exists(&target.cwd, &target.session_id)
                        {
                            failures.push(format!(
                                "Tab {} no longer has session {}",
                                target.tab_id, target.session_id
                            ));
                            continue;
                        }
                        match service.agent_restart(&target.tab_id, false, 10) {
                            Ok(result)
                                if restart_kept_session(
                                    &target.session_id,
                                    result.session_id.as_deref(),
                                ) => {}
                            Ok(_) => failures
                                .push(format!("Tab {} resumed a different session", target.tab_id)),
                            Err(error) => failures
                                .push(format!("Tab {} could not resume: {error:?}", target.tab_id)),
                        }
                    }
                    if failures.is_empty() {
                        service.create_notification(
                            format!("{} updated", offer.provider.display_name()),
                            Some(format!(
                                "Updated to version {}.",
                                offer.version.latest_version
                            )),
                            Some("info"),
                        );
                    } else {
                        notify_owner(
                            &service,
                            format!("{} update needs attention", offer.provider.display_name()),
                            failures.join("; "),
                        );
                    }
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

fn restart_kept_session(expected: &str, actual: Option<&str>) -> bool {
    actual == Some(expected)
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
    let mut process = if cfg!(windows) {
        let mut process = Command::new("cmd.exe");
        process.args(["/D", "/S", "/C", command]);
        process
    } else {
        let mut process = Command::new("sh");
        process.args(["-lc", command]);
        process
    };
    codeterm_util::process_util::no_window_std(&mut process);
    let result = process
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "update command exited with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ))
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
    use super::{update_gate, UpdateGate};
    use codeterm_types::pty::TabStatus;

    #[test]
    fn codex_update_waits_for_consent_and_idle_agents() {
        assert_eq!(
            update_gate(false, &[Some(TabStatus::Idle)]),
            UpdateGate::NoConsent
        );
        assert_eq!(
            update_gate(true, &[Some(TabStatus::Working)]),
            UpdateGate::WaitForIdle
        );
        assert_eq!(
            update_gate(true, &[Some(TabStatus::Idle)]),
            UpdateGate::Ready
        );
        assert_eq!(update_gate(true, &[None]), UpdateGate::WaitForIdle);
    }

    #[test]
    fn provider_update_requires_the_same_session_after_restart() {
        assert!(super::restart_kept_session("session-a", Some("session-a")));
        assert!(!super::restart_kept_session("session-a", Some("session-b")));
        assert!(!super::restart_kept_session("session-a", None));
    }
}
