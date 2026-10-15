use codeterm_service_core::multiplexer::commands::MuxState;
use tauri::Manager;

pub(crate) fn handle_window_event(
    window: &codeterm_host_runtime::app_runtime::Window,
    event: &tauri::WindowEvent,
) {
    if matches!(event, tauri::WindowEvent::Focused(true)) && window.label() == "main" {
        if let Some(service) = window
            .app_handle()
            .try_state::<codeterm_service_core::service::ServiceState>()
        {
            let service = service.inner().clone();
            tauri::async_runtime::spawn(async move {
                let _ = service
                    .refresh_billing(codeterm_billing::RefreshEvent::Focus)
                    .await;
            });
        }
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() != "main" {
            return;
        }

        let app = window.app_handle();
        if let (Some(mux), Some(service)) = (
            app.try_state::<MuxState>(),
            app.try_state::<codeterm_service_core::service::ServiceState>(),
        ) {
            if let Err(error) =
                codeterm_service_core::service::session_persist::submit_session_snapshot_with_intent(
                    service.inner(),
                    mux.inner(),
                    codeterm_service_core::session::store::SaveIntent::PreserveDesk,
                )
            {
                log::error!("Failed to submit close-to-tray session snapshot: {error}");
            } else {
                log::info!("Session snapshot submitted (close-to-tray)");
            }
        }

        api.prevent_close();
        let _ = window.hide();
        log::info!("Window hidden to tray");
    }
}
