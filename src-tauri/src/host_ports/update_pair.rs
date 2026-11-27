use std::sync::Arc;

use codeterm_client_shell::application_host::{ServerStop, ServerStopError, ServerStopState};
use codeterm_update::update_state::{detect_install_method, UpdatePhase, UpdateState};
use tauri::{Emitter, Manager};

use super::native_client::{ExactServerShutdownResult, NativeHostClientSlot};
use super::ServerApiAttachmentSlot;
use crate::crash;

/// The native client owns the in-app updater; its state lives here, not in the attached server.
pub(crate) fn install(app: &codeterm_client_shell::app_runtime::AppHandle) {
    let events = app.clone();
    let _ = codeterm_update::update_state::STATUS_EMITTER.set(Box::new(move |status| {
        if let Err(error) = events.emit("update-status-changed", status) {
            log::debug!("[updater] native status event could not be emitted: {error}");
        }
    }));
    let update_state: codeterm_update::UpdateStateHandle =
        Arc::new(parking_lot::Mutex::new(UpdateState {
            install_method: detect_install_method(),
            last_check: None,
            last_check_time: None,
            channel: Default::default(),
            phase: UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        }));
    if let Some(error) = codeterm_update::debian::failure(&codeterm_paths::data_dir()) {
        update_state.lock().set_error(error);
    }
    app.manage(update_state);
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn_blocking(|| {
        codeterm_update::debian::cleanup(&std::env::temp_dir());
        codeterm_update::debian::cleanup(&codeterm_paths::data_dir().join("state/update-handoffs"));
        if let Err(error) = codeterm_client_shell::refresh_owned_linux_unit() {
            log::warn!("[updater] generated user unit refresh failed: {error}");
        }
    });
    #[cfg(target_os = "macos")]
    {
        let label = app.config().identifier.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) = codeterm_client_shell::refresh_owned_macos_agent(&label) {
                log::warn!("[autostart] generated LaunchAgent refresh failed: {error}");
            }
        });
    }
    app.manage(Arc::new(
        super::native_client::update::UpdateOperation::default(),
    ));
    app.manage(NativeHostClientSlot::default());
    app.manage(
        codeterm_client_shell::application_host::NativeUpdateApplyState(Arc::new(
            super::native_client::update::AttachedNativeUpdateApply {
                app: app.clone(),
                slot: app.state::<NativeHostClientSlot>().inner().clone(),
            },
        )),
    );
    app.manage(ServerStopState(Arc::new(AttachedServerStop {
        app: app.clone(),
    })));
}

struct AttachedServerStop {
    app: codeterm_client_shell::app_runtime::AppHandle,
}

#[async_trait::async_trait]
impl ServerStop for AttachedServerStop {
    async fn ensure_attached(&self) -> Result<(), String> {
        let _ = super::native_client::recovery::ensure_server_api_attachment(&self.app).await?;
        self.install_identity().map(|_| ())
    }

    async fn reserve_install_shutdown(
        &self,
        request: codeterm_update::holders::ReserveHolders,
    ) -> Result<String, String> {
        let api = super::native_client::recovery::ensure_server_api_attachment(&self.app).await?;
        let response = api
            .client
            .post(format!("{}/api/update/reserve", api.api_origin()))
            .bearer_auth(api.token())
            .json(&request)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(response.text().await.map_err(|e| e.to_string())?);
        }
        response.json().await.map_err(|e| e.to_string())
    }

    async fn release_install_shutdown(&self, id: &str) -> Result<(), String> {
        let api = super::native_client::recovery::ensure_server_api_attachment(&self.app).await?;
        let response = api
            .client
            .post(format!("{}/api/update/release", api.api_origin()))
            .bearer_auth(api.token())
            .json(id)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(response.text().await.map_err(|e| e.to_string())?)
        }
    }

    async fn describe_install_holders(
        &self,
        processes: Vec<codeterm_update::holders::HolderProcess>,
    ) -> Result<Vec<codeterm_update::holders::HolderTab>, String> {
        let api = super::native_client::recovery::ensure_server_api_attachment(&self.app).await?;
        api.client
            .post(format!("{}/api/update/holders", api.api_origin()))
            .bearer_auth(api.token())
            .json(&processes)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(target_os = "linux")]
    fn debian_quit_target(&self) -> Result<codeterm_update::debian::handoff::QuitTarget, String> {
        let api = self
            .app
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
            .ok_or("the server attachment is unavailable")?;
        let client = codeterm_process_identity::PinnedProcess::open(std::process::id())
            .and_then(|pin| pin.provenance())
            .map_err(|e| e.to_string())?;
        let identity = codeterm_paths::instance_identity::ResolvedInstancePaths::resolve(
            &api.ready().identity.data,
            &api.ready().identity.config,
        )
        .map_err(|e| e.to_string())?;
        let registry =
            codeterm_paths::instance_identity::ownership_registry().map_err(|e| e.to_string())?;
        let instance_locks = identity
            .ownership_lock_paths(&registry)
            .map_err(|e| e.to_string())?;
        Ok(codeterm_update::debian::handoff::QuitTarget {
            ready: api.ready().clone(),
            client,
            instance_locks,
            token: api.token().into(),
        })
    }
    fn install_identity(
        &self,
    ) -> Result<codeterm_client_shell::application_host::AttachedServerIdentity, String> {
        let api = self
            .app
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
            .ok_or("the server attachment is unavailable")?;
        let expected = &api.ready().process;
        let pinned = codeterm_process_identity::PinnedProcess::open(expected.pid)
            .map_err(|e| e.to_string())?;
        if pinned.provenance().map_err(|e| e.to_string())? != *expected {
            return Err("the attached server process identity changed".into());
        }
        Ok(
            codeterm_client_shell::application_host::AttachedServerIdentity {
                pid: expected.pid,
                created: expected.created.clone(),
            },
        )
    }

    async fn stop_server(&self) -> Result<(), ServerStopError> {
        let refused = |reason: String| ServerStopError {
            dispatched: false,
            reason,
        };
        super::native_client::recovery::ensure_server_api_attachment(&self.app)
            .await
            .map_err(refused)?;
        let native = self
            .app
            .try_state::<NativeHostClientSlot>()
            .map(|slot| slot.inner().clone());
        let api = self
            .app
            .try_state::<ServerApiAttachmentSlot>()
            .map(|slot| slot.inner().clone());
        let (Some(native), Some(api)) = (native, api) else {
            return Err(refused("the server attachment is unavailable".into()));
        };
        let ticket = crash::exit::begin_client_exit(crash::exit::ClientExitReason::ExplicitQuit)
            .map_err(|_| refused("another quit is already in progress".into()))?;
        match native.request_exact_server_shutdown(&api, None).await {
            ExactServerShutdownResult::Confirmed { .. } => {
                crash::exit::confirm_client_exit(ticket);
                Ok(())
            }
            ExactServerShutdownResult::RefusedBeforeDispatch(reason) => {
                crash::exit::refuse_client_exit_before_dispatch(ticket);
                Err(refused(format!("server stop refused: {reason:?}")))
            }
            ExactServerShutdownResult::UncertainAfterDispatch(reason) => {
                crash::exit::mark_client_exit_uncertain(ticket);
                Err(ServerStopError {
                    dispatched: true,
                    reason: format!("server exit unconfirmed: {reason:?}"),
                })
            }
        }
    }
}
