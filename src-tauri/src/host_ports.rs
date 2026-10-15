use std::path::PathBuf;

use codeterm_service_core::ports::host::{Shell, ShellError, UiEvents, WebviewRequest};
use serde_json::Value;
use tauri::{Emitter, Manager};

type AppHandle = codeterm_host_runtime::app_runtime::AppHandle;

pub struct TauriUiEvents(pub AppHandle);

impl UiEvents for TauriUiEvents {
    fn emit(&self, event: &str, payload: Value) {
        codeterm_host_runtime::events::emit_everywhere(&self.0, event, payload);
    }

    fn emit_native(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
    }

    fn emit_to(&self, target: &str, event: &str, payload: Value) {
        let _ = self.0.emit_to(target, event, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    #[test]
    fn ui_events_reach_desktop_and_web_subscribers() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let events = std::sync::Arc::new(codeterm_events::EventBroadcaster::new());
        let mut web = events.subscribe();
        assert!(app.manage(events));
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen("host-probe", move |event| {
            sender.send(event.payload().to_owned()).unwrap();
        });
        let ui = TauriUiEvents(app.handle().clone());
        let payload = serde_json::json!({ "probe": [1, 2] });
        ui.emit("host-probe", payload.clone());
        let desktop: Value = serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
        assert_eq!(desktop, payload);
        match web.try_recv().unwrap().event {
            codeterm_events::SystemEvent::Generic { event_type, data } => {
                assert_eq!(event_type, "host-probe");
                assert_eq!(data, payload);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        let (native_sender, native_receiver) = std::sync::mpsc::channel();
        app.listen("native-probe", move |event| {
            native_sender.send(event.payload().to_owned()).unwrap();
        });
        ui.emit_native("native-probe", serde_json::json!({ "native": true }));
        let native: Value = serde_json::from_str(&native_receiver.try_recv().unwrap()).unwrap();
        assert_eq!(native, serde_json::json!({ "native": true }));
        assert!(web.try_recv().is_err());
        ui.emit_to("main", "targeted-probe", Value::Null);
        assert!(web.try_recv().is_err());
    }
}

pub struct TauriShell(pub AppHandle);

fn failed(error: impl std::fmt::Display) -> ShellError {
    ShellError::Failed(error.to_string())
}

impl TauriShell {
    fn on_main<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ShellError> + Send + 'static,
    ) -> Result<T, ShellError> {
        let app = self.0.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        self.0
            .run_on_main_thread(move || {
                let _ = sender.send(action(app));
            })
            .map_err(failed)?;
        receiver.recv().map_err(failed)?
    }
}

impl Shell for TauriShell {
    fn resource_dir(&self) -> Result<PathBuf, ShellError> {
        self.0.path().resource_dir().map_err(failed)
    }

    fn focus_window(&self, label: &str) -> Result<(), ShellError> {
        let label = label.to_owned();
        self.on_main(move |app| {
            if let Some(window) = app.get_webview_window(&label) {
                window.show().map_err(failed)?;
                window.unminimize().map_err(failed)?;
                window.set_focus().map_err(failed)?;
            }
            Ok(())
        })
    }

    fn window_exists(&self, label: &str) -> bool {
        let label = label.to_owned();
        self.on_main(move |app| Ok(app.get_webview_window(&label).is_some()))
            .unwrap_or(false)
    }

    fn os_notify(&self, title: &str, body: Option<&str>) -> Result<(), ShellError> {
        crate::os_notify::_send_os_notification(&self.0, title, body);
        Ok(())
    }

    fn toast_permission(&self, enabling: Option<bool>) -> String {
        match enabling {
            Some(enabling) => crate::os_notify::establish_toast_permission(&self.0, enabling),
            None => crate::os_notify::toast_permission(&self.0),
        }
        .to_string()
    }

    fn open_webview(&self, request: WebviewRequest) -> Result<(), ShellError> {
        let url = request.url.parse().map_err(failed)?;
        self.on_main(move |app| {
            tauri::WebviewWindowBuilder::new(&app, request.label, tauri::WebviewUrl::External(url))
                .title(request.title)
                .inner_size(request.width, request.height)
                .build()
                .map_err(failed)?;
            Ok(())
        })
    }

    fn current_exe(&self) -> Result<PathBuf, ShellError> {
        tauri::utils::platform::current_exe().map_err(failed)
    }

    fn config_dir(&self) -> Result<PathBuf, ShellError> {
        self.0.path().config_dir().map_err(failed)
    }
}
