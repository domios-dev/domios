use codeterm_client_shell::app_runtime::AppHandle;
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{Manager, WebviewWindowBuilder};

pub(crate) fn show(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let config = super::frontend_shell::initial_document::starting_window_config(app.config())?;
    let starting_url = super::frontend_shell::starting_url()?;
    let handle = app.clone();
    let paint = Arc::new(Mutex::new(
        super::frontend_shell::initial_document::StartingPaint::default(),
    ));
    let page_paint = paint.clone();
    let window = WebviewWindowBuilder::from_config(app, &config)?
        .initialization_script(super::frontend_shell::initial_document::STARTING_PAINT_SCRIPT)
        .on_page_load(move |window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                page_paint.lock().begin();
                if let Err(error) = window.hide() {
                    log::debug!("[startup] starting window concealment failed: {error}");
                }
            } else {
                let generation = page_paint.lock().generation();
                if let Err(error) =
                    window.eval(&format!("window.__DOMIOS_STARTING_PAINT__?.({generation})"))
                {
                    log::debug!("[startup] starting document paint request failed: {error}");
                }
            }
        })
        .on_navigation(move |url| {
            if url.scheme() == super::frontend_shell::initial_document::STARTING_PAINT_SCHEME {
                if paint.lock().acknowledge(url) {
                    if let Some(window) = handle.get_webview_window("starting") {
                        if let Err(error) = window.show() {
                            log::debug!("[startup] painted starting window reveal failed: {error}");
                        }
                    }
                }
                false
            } else if url.scheme() == super::frontend_shell::SHELL_ACTION_SCHEME {
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
