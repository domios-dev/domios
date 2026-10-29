use codeterm_client_shell::app_runtime::AppHandle;
use tauri::{Manager, WebviewWindowBuilder};

pub(crate) fn show(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = super::frontend_shell::initial_document::window_config(app.config())?;
    config.label = "starting".into();
    config.visible = true;
    let starting_url = super::frontend_shell::starting_url()?;
    let handle = app.clone();
    let window = WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(move |url| {
            if url.scheme() == super::frontend_shell::SHELL_ACTION_SCHEME {
                super::frontend_shell::run_shell_action(&handle, url);
                false
            } else {
                url == &starting_url || url.scheme() == "data"
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
