use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use codeterm_plugin::provider::{AgentProvider, ProviderUpdateAvailability};
use parking_lot::Mutex;

use super::ServiceContext;
use state::Admission;

mod command;
mod restarts;
mod state;
mod storage;
mod watch;

pub(crate) const IDLE_REFUSAL: &str = "provider update restart deferred until tab is idle";

pub(crate) fn restart_eligible(service: &ServiceContext, tab: &str) -> bool {
    let provider = service
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone());
    let Some(provider) = provider else {
        return false;
    };
    let Ok(state) = storage::read() else {
        return false;
    };
    if state.runs.values().any(|run| {
        run.provider == provider
            && matches!(
                run.outcome,
                state::RunOutcome::Running | state::RunOutcome::UnstoppedFailure(_)
            )
    }) {
        return false;
    }
    restarts::readiness(service, tab).can_restart()
}

struct Update {
    provider: Arc<dyn AgentProvider>,
    version: ProviderUpdateAvailability,
    command: String,
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static WAITING: LazyLock<Mutex<HashMap<String, Update>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn check_available(service: Arc<ServiceContext>, api_port: u16, allow_non_prod: bool) {
    let instance_override = std::env::var("CODETERM_INSTANCE").ok();
    let allowed =
        update_instance_name(instance_override.as_deref(), api_port, allow_non_prod).is_some();
    ENABLED.store(allowed, Ordering::Release);
    if allowed {
        watch::start(service.as_ref());
        restarts::schedule_async(service.as_ref(), None);
    }
    let providers = service.agent_registry.all_provider_arcs();
    spawn_utility("provider-update-check", move || {
        for provider in providers {
            if !provider.supports_update_check() {
                continue;
            }
            let Some(command) = provider.update_command(command::platform_key()) else {
                continue;
            };
            let Some(version) = provider.check_for_update() else {
                continue;
            };
            if allowed {
                start_update(
                    service.as_ref(),
                    Update {
                        provider,
                        version,
                        command,
                    },
                );
            } else {
                log::info!(
                    "[provider-update] {} {} -> {} available; instance updates are disabled",
                    provider.display_name(),
                    version.current_version,
                    version.latest_version
                );
            }
        }
    });
}

pub(crate) fn coordinates_prompt(service: &ServiceContext, provider: &str) -> bool {
    ENABLED.load(Ordering::Acquire)
        && service
            .agent_registry
            .provider_arc(provider)
            .is_some_and(|provider| provider.update_command(command::platform_key()).is_some())
}

pub(crate) fn manual_prompt_observed(
    service: &ServiceContext,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let provider_id = service
        .mux
        .read()
        .get_tab(&snapshot.tab_id)
        .and_then(|tab| {
            (tab.agent_type.as_deref() == Some(snapshot.agent_type.as_str())
                && tab.agent_session_id == snapshot.session_id)
                .then(|| tab.agent_type.clone())
                .flatten()
        });
    let Some(provider) = provider_id.and_then(|id| service.agent_registry.provider_arc(&id)) else {
        return;
    };
    let Some(command) = provider.update_command(command::platform_key()) else {
        return;
    };
    let screen = snapshot.screen_text.clone();
    let service = service.clone();
    spawn_utility("provider-update-prompt", move || {
        if let Some(version) = provider
            .update_prompt_availability(&screen)
            .or_else(|| provider.check_for_update())
        {
            start_update(
                &service,
                Update {
                    provider,
                    version,
                    command,
                },
            );
        } else {
            log::warn!(
                "[provider-update] classified prompt has no provider-declared target version"
            );
        }
    });
}

fn update_instance_name(instance: Option<&str>, port: u16, allow_non_prod: bool) -> Option<String> {
    if let Some(name) = instance.map(str::trim).filter(|name| !name.is_empty()) {
        return allow_non_prod.then(|| name.into());
    }
    if port == 7685 {
        return Some("production".into());
    }
    allow_non_prod.then(|| format!("instance on port {port}"))
}

fn start_update(service: &ServiceContext, update: Update) {
    let provider_id = update.provider.id().to_string();
    let key = format!("{provider_id}:{}", update.version.latest_version);
    let tabs = restarts::provider_tabs(service, &provider_id);
    let admission = storage::transact(|state| {
        let admission = state.begin(
            &provider_id,
            &update.version.current_version,
            &update.version.latest_version,
        );
        state.enroll(&key, &tabs);
        admission
    });
    match admission {
        Ok(Admission::Duplicate) => {
            restarts::schedule(service, None);
            return;
        }
        Ok(Admission::ProviderBusy) => {
            WAITING.lock().insert(provider_id, update);
            return;
        }
        Err(error) => {
            log::error!("[provider-update] durable admission failed: {error}");
            return;
        }
        Ok(Admission::Started) => {}
    }
    let service = service.clone();
    let failure_service = service.clone();
    let failure_key = key.clone();
    let builder = std::thread::Builder::new().name("provider-update-install".into());
    let spawned = builder.spawn(move || install(service, key, provider_id, update));
    if let Err(error) = spawned {
        let _ = storage::transact(|state| {
            state.finish(
                &failure_key,
                Err(format!("installer utility thread failed: {error}")),
                &[],
            )
        });
        restarts::schedule(&failure_service, None);
    }
}

fn install(service: ServiceContext, key: String, provider_id: String, update: Update) {
    let mut child_stopped = true;
    let mut result = if journal(
        &provider_id,
        Some(&update.version.latest_version),
        None,
        "update_started",
        "provider_update_command",
    ) {
        command::run_update_command(&update.command).map_err(|failure| {
            child_stopped = failure.child_stopped;
            failure.reason
        })
    } else {
        Err("update withheld because the Supporter journal is unavailable".into())
    };
    if !journal(
        &provider_id,
        Some(&update.version.latest_version),
        None,
        if result.is_ok() {
            "update_succeeded"
        } else {
            "update_failed"
        },
        "provider_update_command",
    ) && result.is_ok()
    {
        result = Err(
            "update command succeeded but its journal write failed; automatic restarts withheld"
                .into(),
        );
    }
    let tabs = restarts::provider_tabs(&service, &provider_id);
    if let Err(error) = storage::transact(|state| {
        state.finish(&key, result, &tabs);
        if !child_stopped {
            state.block_unstopped(&key);
        }
    }) {
        log::error!("[provider-update] completion persistence failed; restart withheld: {error}");
    }
    restarts::schedule(&service, None);
    process_waiting(&service);
}

fn process_waiting(service: &ServiceContext) {
    let updates = std::mem::take(&mut *WAITING.lock());
    for (_, update) in updates {
        start_update(service, update);
    }
}

pub(crate) fn task_state_changed(service: &ServiceContext) {
    if ENABLED.load(Ordering::Acquire) {
        restarts::schedule_async(service, None);
    }
}

pub(crate) fn tab_became_idle(service: &ServiceContext, tab_id: &str) {
    if ENABLED.load(Ordering::Acquire) {
        restarts::schedule_async(service, Some(tab_id.to_string()));
    }
}

pub(crate) fn background_work_changed(service: &ServiceContext, tab_id: &str) {
    if ENABLED.load(Ordering::Acquire) {
        restarts::schedule_async(service, Some(tab_id.into()));
    }
}

fn spawn_utility(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::error!("[provider-update] utility thread failed: {error}");
    }
}

pub(crate) fn record_safe_prompt_answer(provider: &str, tab: &str) {
    journal(
        provider,
        None,
        Some(tab),
        "safe_choice_answered",
        "provider_declared_update_safe_choice",
    );
}

fn journal(
    provider: &str,
    target: Option<&str>,
    tab: Option<&str>,
    label: &str,
    reason: &'static str,
) -> bool {
    super::supporter::runtime()
        .provider_update_action(
            provider,
            target,
            super::supporter::ActionRecord {
                kind: super::supporter::VerdictKind::TabState,
                tab,
                label,
                confidence: 1.0,
                action: super::supporter::Action::Recovery,
                reason,
                asked: Some(super::supporter::update_questions(label)),
            },
        )
        .is_ok()
}

fn notify_owner(
    service: &ServiceContext,
    _key: &str,
    title: String,
    body: String,
    failed: bool,
) -> String {
    service.log_system_notice(
        title,
        Some(body),
        Some(if failed { "warning" } else { "info" }),
    )
}

#[cfg(test)]
mod tests;

#[cfg(windows)]
pub(crate) use command::process::ProcessChild;
pub(crate) use command::process::ProcessTree;
