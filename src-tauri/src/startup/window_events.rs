use crate::multiplexer::commands::MuxState;
use crate::session::SessionState;
use tauri::Manager;

pub(crate) fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() != "main" {
            return;
        }

        let app = window.app_handle();
        if let (Some(mux), Some(_session)) =
            (app.try_state::<MuxState>(), app.try_state::<SessionState>())
        {
            if let Err(error) = crate::service::session_persist::submit_session_snapshot_with_intent(
                app,
                mux.inner(),
                crate::session::store::SaveIntent::PreserveDesk,
            ) {
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
