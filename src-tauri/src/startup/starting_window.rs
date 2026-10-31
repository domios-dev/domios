use codeterm_client_shell::app_runtime::AppHandle;
use std::sync::Arc;
use tauri::{Manager, WebviewWindowBuilder};

pub(crate) fn show(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = super::frontend_shell::initial_document::starting_window_config(app.config())?;
    super::frontend_shell::configure_window_browser_arguments(app, &mut config)?;
    let starting_url = super::frontend_shell::starting_url()?;
    let handle = app.clone();
    let layout = Arc::new(super::frontend_shell::initial_document::StartingLayout::default());
    let initial_generation = layout.begin();
    let page_layout = layout.clone();
    let navigation_layout = layout.clone();
    let window = WebviewWindowBuilder::from_config(app, &config)?
        .initialization_script(super::frontend_shell::initial_document::STARTING_READY_SCRIPT)
        .on_page_load(move |window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                let generation = page_layout.begin();
                super::frontend_shell::initial_document::supervise_starting(
                    window.app_handle().clone(),
                    page_layout.clone(),
                    generation,
                );
                if let Err(error) = window.hide() {
                    log::debug!("[startup] starting window concealment failed: {error}");
                    if page_layout.request(generation, || Err::<(), _>(error)) {
                        super::frontend_shell::initial_document::show_layout_failure(
                            window.app_handle(),
                            page_layout.clone(),
                            generation,
                        );
                    }
                }
            } else {
                let generation = page_layout.generation();
                if page_layout.request(generation, || {
                    window
                        .eval(&format!("window.__DOMIOS_STARTING_READY__?.({generation})"))
                        .inspect_err(|error| {
                            log::debug!(
                                "[startup] starting document layout request failed: {error}"
                            )
                        })
                }) {
                    super::frontend_shell::initial_document::show_layout_failure(
                        window.app_handle(),
                        page_layout.clone(),
                        generation,
                    );
                }
            }
        })
        .on_navigation(move |url| {
            if url.scheme() == super::frontend_shell::initial_document::STARTING_READY_SCHEME {
                if let Some(generation) = navigation_layout.reveal(url, || {
                    let window = handle
                        .get_webview_window("starting")
                        .ok_or_else(|| "Starting window is no longer available".to_string())?;
                    window.show().map_err(|error| {
                        log::debug!("[startup] prepared starting window reveal failed: {error}");
                        error.to_string()
                    })
                }) {
                    super::frontend_shell::initial_document::show_layout_failure(
                        &handle,
                        navigation_layout.clone(),
                        generation,
                    );
                }
                false
            } else if url.scheme()
                == super::frontend_shell::initial_document::STARTING_UNAVAILABLE_SCHEME
            {
                if let Some(generation) = navigation_layout.refuse(url) {
                    super::frontend_shell::initial_document::show_layout_failure(
                        &handle,
                        navigation_layout.clone(),
                        generation,
                    );
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
    super::frontend_shell::initial_document::supervise_starting(
        app.clone(),
        layout.clone(),
        initial_generation,
    );
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            layout.retire();
        }
    });
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
