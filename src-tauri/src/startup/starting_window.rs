use codeterm_client_shell::app_runtime::AppHandle;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) fn show(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("Main window configuration is missing")?;
    config.label = "starting".into();
    config.url = WebviewUrl::CustomProtocol(super::frontend_shell::starting_url()?);
    config.visible = true;
    let handle = app.clone();
    let window = WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(move |url| {
            if url.scheme() == super::frontend_shell::SHELL_ACTION_SCHEME {
                super::frontend_shell::run_shell_action(&handle, url);
                false
            } else {
                url.scheme() == "data"
            }
        })
        .build()?;
    super::window::setup_window_styling(&window);
    crate::host_ports::native_client::startup_reached(
        crate::host_ports::native_client::ClientStartupStage::StartingWindow,
    );
    Ok(())
}

pub(crate) fn dismiss(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("starting") {
        if let Err(error) = window.destroy() {
            log::debug!("[startup] starting window dismissal failed: {error}");
        }
    }
}
