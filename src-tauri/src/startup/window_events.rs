use crate::host_ports::ServerApiAttachmentSlot;
use tauri::Manager;

pub(crate) fn handle_window_event(
    window: &codeterm_client_shell::app_runtime::Window,
    event: &tauri::WindowEvent,
) {
    if matches!(event, tauri::WindowEvent::Focused(true)) && window.label() == "main" {
        if let Some(attachment) = window
            .app_handle()
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
        {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = attachment.refresh_plan_on_focus().await {
                    log::debug!("[desktop] server focus refresh failed: {error}");
                }
            });
        }
    }
    if let tauri::WindowEvent::CloseRequested { .. } = event {
        if window.label() != "main" {
            return;
        }
        if !super::desktop_lifecycle::close_requested(window) {
            return;
        }

        let app = window.app_handle();
        if let Some(attachment) = app
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
        {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = attachment.save_session().await {
                    log::debug!("[desktop] server close snapshot request failed: {error}");
                }
            });
        }
    }
}
