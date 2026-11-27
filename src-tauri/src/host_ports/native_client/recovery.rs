use super::{
    bootstrap, initialize_client_once, AppHandle, NativeClientInitializationGate,
    NativeClientInstallInputs, NativeClientStatus, NativeHostClient, NativeHostClientSlot,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::Manager;

const MAX_SILENT_REATTACHES: usize = 3;
const REATTACH_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DisconnectInput {
    pub was_current_client: bool,
    pub api_generation_matches: bool,
    pub client_exit_pending: bool,
    pub server_process_alive: bool,
    pub auto_reattach_allowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DisconnectDecision {
    IgnoreStale,
    ClearForShutdown,
    SilentReattachThenEscalate,
    ClearAndEscalate,
}

pub(super) fn decide_disconnect_recovery(input: DisconnectInput) -> DisconnectDecision {
    if !input.was_current_client || !input.api_generation_matches {
        DisconnectDecision::IgnoreStale
    } else if input.client_exit_pending {
        DisconnectDecision::ClearForShutdown
    } else if input.server_process_alive && input.auto_reattach_allowed {
        DisconnectDecision::SilentReattachThenEscalate
    } else {
        DisconnectDecision::ClearAndEscalate
    }
}

pub(super) struct ReattachBudget {
    attempts: [Option<Instant>; MAX_SILENT_REATTACHES],
}

impl ReattachBudget {
    pub(super) const fn new() -> Self {
        Self {
            attempts: [None; MAX_SILENT_REATTACHES],
        }
    }

    pub(super) fn allow(&mut self, now: Instant) -> bool {
        let mut slot = None;
        for (index, entry) in self.attempts.iter_mut().enumerate() {
            if entry
                .is_some_and(|recorded| now.saturating_duration_since(recorded) >= REATTACH_WINDOW)
            {
                *entry = None;
            }
            if entry.is_none() && slot.is_none() {
                slot = Some(index);
            }
        }
        let Some(index) = slot else {
            return false;
        };
        self.attempts[index] = Some(now);
        true
    }
}

static SILENT_REATTACH_BUDGET: parking_lot::Mutex<ReattachBudget> =
    parking_lot::Mutex::new(ReattachBudget::new());

pub(super) enum ReattachOutcome {
    Attached,
    AlreadyInProgress,
}

pub(super) async fn retry_inputs(
    inputs: NativeClientInstallInputs,
) -> Result<NativeClientInstallInputs, String> {
    #[cfg(debug_assertions)]
    if matches!(
        inputs.launch_options.instance,
        codeterm_types::server_bootstrap::InstanceSelector::Development { .. }
    ) {
        return tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(inputs.trusted_resource_root.join("server-component.json"))
                .map_err(|error| error.to_string())?;
            let manifest: codeterm_types::server_bootstrap::ServerComponentManifest =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if !same_client(&inputs.manifest, &manifest) {
                return Err("DEV component changed the running native client identity".into());
            }
            Ok(NativeClientInstallInputs { manifest, ..inputs })
        })
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(inputs)
}

#[cfg(any(debug_assertions, test))]
fn same_client(
    previous: &codeterm_types::server_bootstrap::ServerComponentManifest,
    current: &codeterm_types::server_bootstrap::ServerComponentManifest,
) -> bool {
    previous.schema == current.schema
        && previous.api_version == current.api_version
        && previous.bootstrap_protocol == current.bootstrap_protocol
        && previous.client_identity == current.client_identity
        && previous.server_executable == current.server_executable
}

pub(super) async fn disconnected(slot: &NativeHostClientSlot, client: &Arc<NativeHostClient>) {
    let was_current_client = slot.clear_if_current(client);
    let Some(api) = client
        .app
        .try_state::<super::super::ServerApiAttachmentSlot>()
        .map(|state| state.inner().clone())
    else {
        return;
    };
    let generation = client.api_generation();
    let api_generation_matches = api.matches_generation(generation, &client.ready);
    let client_exit_pending = crate::crash::exit::is_client_exit_pending();
    let server_process_alive =
        if was_current_client && api_generation_matches && !client_exit_pending {
            let process = client.server_process.clone();
            tokio::task::spawn_blocking(move || process.ensure_alive().is_ok())
                .await
                .unwrap_or(false)
        } else {
            false
        };
    let auto_reattach_allowed =
        server_process_alive && SILENT_REATTACH_BUDGET.lock().allow(Instant::now());
    match decide_disconnect_recovery(DisconnectInput {
        was_current_client,
        api_generation_matches,
        client_exit_pending,
        server_process_alive,
        auto_reattach_allowed,
    }) {
        DisconnectDecision::IgnoreStale => {}
        DisconnectDecision::ClearForShutdown => {
            api.clear_if_generation(generation, &client.ready);
        }
        DisconnectDecision::SilentReattachThenEscalate => {
            match reattach_live_server(client.app.clone()).await {
                Ok(ReattachOutcome::Attached) => {
                    crate::startup::frontend_shell::rebind_active_view(&client.app);
                    log::info!("[native-client] automatically re-attached to live server");
                }
                Ok(ReattachOutcome::AlreadyInProgress) => {}
                Err(error) => {
                    log::warn!("[native-client] silent re-attach failed: {error}");
                    if api.clear_if_generation(generation, &client.ready)
                        || (!slot.has_live_client() && api.clear_if_current(&client.ready))
                    {
                        crate::startup::frontend_shell::server_disconnected(
                            client.app.clone(),
                            client.ready.clone(),
                            client.server_process.clone(),
                        );
                    }
                }
            }
        }
        DisconnectDecision::ClearAndEscalate => {
            if api.clear_if_generation(generation, &client.ready) {
                crate::startup::frontend_shell::server_disconnected(
                    client.app.clone(),
                    client.ready.clone(),
                    client.server_process.clone(),
                );
            }
        }
    }
}

// A named boxed Send future breaks the initialize -> disconnect -> reattach -> initialize opaque-type cycle.
pub(super) fn reattach_live_server(
    app: AppHandle,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ReattachOutcome, String>> + Send>> {
    Box::pin(reattach_live_server_inner(app))
}

async fn reattach_live_server_inner(app: AppHandle) -> Result<ReattachOutcome, String> {
    let Some(gate) = app
        .try_state::<NativeClientInitializationGate>()
        .map(|state| state.inner().clone())
    else {
        return Err("the server attachment is unavailable".into());
    };
    let Some(_permit) = gate.acquire() else {
        return Ok(ReattachOutcome::AlreadyInProgress);
    };
    let inputs = retry_inputs(gate.inputs.as_ref().clone()).await?;
    let initialized = initialize_client_once(app, Arc::new(inputs), false)
        .await
        .map_err(|error| error.message().to_owned())?;
    match initialized.native {
        NativeClientStatus::Attached => {
            gate.attached_once
                .store(true, std::sync::atomic::Ordering::Release);
            Ok(ReattachOutcome::Attached)
        }
        NativeClientStatus::NativeHostUnavailable { message, .. } => Err(message),
    }
}

/// Restores `ServerApiAttachmentSlot` against the still-running server without
/// replacing an active native-host socket mid-update.
pub(in crate::host_ports) async fn ensure_server_api_attachment(
    app: &AppHandle,
) -> Result<super::super::ServerApiAttachment, String> {
    if let Some(api) = app
        .try_state::<super::super::ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current())
    {
        return Ok(api);
    }
    let Some(gate) = app
        .try_state::<NativeClientInitializationGate>()
        .map(|state| state.inner().clone())
    else {
        return Err("the server attachment is unavailable".into());
    };
    let Some(_permit) = gate.acquire() else {
        return app
            .try_state::<super::super::ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
            .ok_or_else(|| "the server attachment is unavailable".into());
    };
    if let Some(api) = app
        .try_state::<super::super::ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current())
    {
        return Ok(api);
    }
    log::info!("[native-client] server API attachment missing; attempting automatic re-attach");
    let inputs = retry_inputs(gate.inputs.as_ref().clone()).await?;
    let verified = tokio::task::spawn_blocking(move || {
        bootstrap::verify_install(
            inputs.manifest,
            inputs.trusted_resource_root,
            inputs.launch_options,
        )
    })
    .await
    .map_err(|error| format!("the server attachment is unavailable ({error})"))?
    .map_err(|error| format!("the server attachment is unavailable ({error})"))?;
    let reply = bootstrap::attach_or_launch(&verified, false)
        .await
        .map_err(|error| format!("the server attachment is unavailable ({error})"))?;
    bootstrap::validate_ready(&verified, &reply.ready)
        .map_err(|error| format!("the server attachment is unavailable ({error})"))?;
    let token = reply
        .api_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            "the server attachment is unavailable (missing API credential)".to_string()
        })?;
    if app
        .try_state::<super::super::ServerApiAttachmentSlot>()
        .is_none()
    {
        let _ = app.manage(super::super::ServerApiAttachmentSlot::default());
    }
    let api_slot = app
        .state::<super::super::ServerApiAttachmentSlot>()
        .inner()
        .clone();
    let generation = api_slot.install(reply.ready.clone(), token)?;
    if let Some(native_slot) = app.try_state::<NativeHostClientSlot>() {
        native_slot.adopt_api_generation_if_same_server(&reply.ready, generation);
    }
    api_slot
        .current()
        .ok_or_else(|| "the server attachment is unavailable".into())
}

pub(super) async fn ensure_native_client_attachment(
    app: &AppHandle,
    slot: &NativeHostClientSlot,
) -> Result<Arc<NativeHostClient>, String> {
    let current = slot.0.read().clone();
    if let Some(client) = current {
        if !client.retired.load(std::sync::atomic::Ordering::Acquire) {
            ensure_server_api_attachment(app).await?;
            return Ok(client);
        }
    }
    match reattach_live_server(app.clone()).await {
        Ok(ReattachOutcome::Attached) => {
            crate::startup::frontend_shell::rebind_active_view(app);
        }
        Ok(ReattachOutcome::AlreadyInProgress) => {}
        Err(error) => {
            return Err(format!("the native host is unavailable ({error})"));
        }
    }
    slot.0
        .read()
        .clone()
        .filter(|client| !client.retired.load(std::sync::atomic::Ordering::Acquire))
        .ok_or_else(|| "the native host is unavailable".into())
}

pub(super) fn should_reattach_on_activate(
    has_live_client: bool,
    has_api_attachment: bool,
    exit_pending: bool,
) -> bool {
    !has_live_client && has_api_attachment && !exit_pending
}

pub(super) fn ensure_attached_in_background(app: &AppHandle) {
    let has_live_client = app
        .try_state::<NativeHostClientSlot>()
        .is_some_and(|slot| slot.has_live_client());
    let has_api_attachment = app
        .try_state::<super::super::ServerApiAttachmentSlot>()
        .is_some_and(|slot| slot.current().is_some());
    if !should_reattach_on_activate(
        has_live_client,
        has_api_attachment,
        crate::crash::exit::is_client_exit_pending(),
    ) {
        return;
    }
    if !SILENT_REATTACH_BUDGET.lock().allow(Instant::now()) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        match reattach_live_server(handle.clone()).await {
            Ok(ReattachOutcome::Attached) => {
                crate::startup::frontend_shell::rebind_active_view(&handle);
                log::info!("[native-client] re-attached native host on window activation");
            }
            Ok(ReattachOutcome::AlreadyInProgress) => {}
            Err(error) => {
                log::warn!("[native-client] activation native re-attach failed: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests;
