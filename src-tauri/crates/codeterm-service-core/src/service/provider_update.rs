use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use codeterm_plugin::provider::{AgentProvider, ProviderUpdateAvailability};
use parking_lot::Mutex;
use tauri::Manager;

use super::ServiceContext;
use state::Admission;

mod command;
mod idle;
mod policy;
use policy::command_for_target;
pub(crate) use policy::compatible_command_for_target;
mod state;
mod storage;
mod watch;

struct Update {
    provider: Arc<dyn AgentProvider>,
    version: ProviderUpdateAvailability,
    command: String,
}

static ENABLED: AtomicBool = AtomicBool::new(false);

pub(crate) fn revision() -> u64 {
    storage::revision()
}
static WAITING: LazyLock<Mutex<HashMap<String, Update>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn check_available(service: Arc<ServiceContext>, api_port: u16, allow_non_prod: bool) {
    let instance_override = std::env::var("CODETERM_INSTANCE").ok();
    let allowed =
        update_instance_name(instance_override.as_deref(), api_port, allow_non_prod).is_some();
    ENABLED.store(allowed, Ordering::Release);
    if allowed {
        watch::start(service.as_ref());
        idle::schedule_async(service.as_ref(), None);
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
    updates_enabled(service)
        && service
            .agent_registry
            .provider_arc(provider)
            .is_some_and(|provider| provider.update_command(command::platform_key()).is_some())
}

pub(crate) fn manual_prompt_observed(
    service: &ServiceContext,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
) {
    if !updates_enabled(service) {
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
            .check_for_update()
            .or_else(|| provider.update_prompt_availability(&screen))
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

fn updates_enabled(service: &ServiceContext) -> bool {
    ENABLED.load(Ordering::Acquire)
        && service
            .app_handle
            .try_state::<crate::config::ConfigState>()
            .is_some_and(|config| config.lock().updater.auto_update_provider_clis)
}

fn start_update(service: &ServiceContext, mut update: Update) {
    let provider_id = update.provider.id().to_string();
    let key = format!("{provider_id}:{}", update.version.latest_version);
    let manifest = update.provider.cli_compatibility();
    let busy = idle::provider_busy(service, &provider_id);
    match policy::may_update_now(
        updates_enabled(service),
        busy,
        manifest.as_ref(),
        &update.version.latest_version,
        &update.command,
    ) {
        policy::UpdateDecision::Deferred => {
            log::debug!("[provider-update] {provider_id}: deferred until provider turns finish");
            WAITING.lock().insert(provider_id, update);
            return;
        }
        decision @ (policy::UpdateDecision::Disabled
        | policy::UpdateDecision::Incompatible
        | policy::UpdateDecision::UnboundedUpdater) => {
            log::info!("[provider-update] {provider_id}: update withheld ({decision:?})");
            return;
        }
        policy::UpdateDecision::Ready => {}
    }
    let resolved_command = match command_for_target(&update.command, &update.version.latest_version)
    {
        Ok(command) => command,
        Err(error) => {
            log::warn!("[provider-update] {provider_id}: {error}");
            return;
        }
    };
    let admission = storage::transact(|state| {
        state.begin(
            &provider_id,
            &update.version.current_version,
            &update.version.latest_version,
        )
    });
    match admission {
        Ok(Admission::Duplicate) => return,
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
    update.command = resolved_command;
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
            )
        });
        idle::schedule(&failure_service, None);
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
            "update command succeeded but its journal write failed; update result unconfirmed"
                .into(),
        );
    }
    let summary = match &result {
        Ok(()) => format!(
            "{provider_id} CLI updater completed for target {}; applies at next launch",
            update.version.latest_version
        ),
        Err(error) => format!("{provider_id} CLI update failed: {error}"),
    };
    log::info!("[provider-update] {summary}");
    if let Err(error) = storage::transact(|state| {
        state.finish(&key, result);
        if !child_stopped {
            state.block_unstopped(&key);
        }
    }) {
        log::error!(
            "[provider-update] completion persistence failed; update result unconfirmed: {error}"
        );
    }
    crate::events::emit_everywhere(
        &service.app_handle,
        "provider-cli-completed",
        serde_json::json!({
            "provider": provider_id, "operation": "update", "summary": summary,
        }),
    );
    process_waiting(&service);
}

fn process_waiting(service: &ServiceContext) {
    let updates = std::mem::take(&mut *WAITING.lock());
    for (_, update) in updates {
        start_update(service, update);
    }
}

pub(crate) fn task_state_changed(service: &ServiceContext) {
    if updates_enabled(service) {
        idle::schedule_async(service, None);
    }
}

pub(crate) fn tab_became_idle(service: &ServiceContext, tab_id: &str) {
    if updates_enabled(service) {
        idle::schedule_async(service, Some(tab_id.to_string()));
    }
}

pub(crate) fn background_work_changed(service: &ServiceContext, tab_id: &str) {
    tab_became_idle(service, tab_id);
}

pub(crate) fn status_summary(provider: &str) -> Option<String> {
    let state = storage::read().ok()?;
    let run = state
        .runs
        .values()
        .filter(|run| run.provider == provider)
        .max_by_key(|run| run.started_at)?;
    Some(match &run.outcome {
        state::RunOutcome::Running => format!("Updating CLI to {}", run.target),
        state::RunOutcome::Succeeded => {
            format!(
                "CLI updater completed for target {}; applies at next launch",
                run.target
            )
        }
        state::RunOutcome::Failed(error) | state::RunOutcome::UnstoppedFailure(error) => {
            format!("CLI update failed: {error}")
        }
    })
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

#[cfg(test)]
mod tests;

#[cfg(windows)]
pub(crate) use command::process::ProcessChild;
pub(crate) use command::process::ProcessTree;
