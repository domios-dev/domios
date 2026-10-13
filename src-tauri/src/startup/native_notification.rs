use crate::app_runtime::{AppHandle, Window};
use tauri::{plugin::Plugin, webview::PageLoadPayload, RunEvent, Runtime, Webview};

pub(crate) struct NativeNotificationPlugin<R: Runtime>(tauri::plugin::TauriPlugin<R>);

pub(crate) fn native_notification<R: Runtime>() -> NativeNotificationPlugin<R> {
    NativeNotificationPlugin(tauri_plugin_notification::init())
}

// The daemon owns notifications; the upstream browser shim eagerly invokes denied IPC.
impl<R: Runtime> Plugin<R> for NativeNotificationPlugin<R> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn initialize(
        &mut self,
        app: &AppHandle<R>,
        config: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.0.initialize(app, config)
    }

    fn window_created(&mut self, window: Window<R>) {
        self.0.window_created(window);
    }

    fn webview_created(&mut self, webview: Webview<R>) {
        self.0.webview_created(webview);
    }

    fn on_navigation(&mut self, webview: &Webview<R>, url: &url::Url) -> bool {
        self.0.on_navigation(webview, url)
    }

    fn on_page_load(&mut self, webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
        self.0.on_page_load(webview, payload);
    }

    fn on_event(&mut self, app: &AppHandle<R>, event: &RunEvent) {
        self.0.on_event(app, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_shell_contract_notifications_have_no_browser_initialization() {
        let upstream = tauri_plugin_notification::init::<crate::app_runtime::Runtime>();
        assert!(upstream.initialization_script_2().is_some());
        let native = native_notification::<crate::app_runtime::Runtime>();
        assert_eq!(native.name(), upstream.name());
        assert!(native.initialization_script().is_none());
        assert!(native.initialization_script_2().is_none());
    }
}
