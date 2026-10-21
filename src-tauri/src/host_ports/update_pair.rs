use std::sync::Arc;

use codeterm_client_shell::application_host::{ServerStop, ServerStopError, ServerStopState};
use codeterm_update::update_state::{detect_install_method, UpdatePhase, UpdateState};
use tauri::Manager;

use super::native_client::{ExactServerShutdownResult, NativeHostClientSlot};
use super::ServerApiAttachmentSlot;
use crate::crash;

/// The native client owns the in-app updater; its state lives here, not in the attached server.
pub(crate) fn install(app: &codeterm_client_shell::app_runtime::AppHandle) {
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
    app.manage(update_state);
    app.manage(ServerStopState(Arc::new(AttachedServerStop {
        app: app.clone(),
    })));
}

struct AttachedServerStop {
    app: codeterm_client_shell::app_runtime::AppHandle,
}

#[async_trait::async_trait]
impl ServerStop for AttachedServerStop {
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
