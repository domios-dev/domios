use codeterm_client_shell::app_runtime::{AppHandle, Window};
use parking_lot::Mutex;
use tauri::Manager;

mod model;
pub(crate) use model::single_instance_enabled;
use model::{Activation, Frontend};

#[derive(Default)]
pub(crate) struct DesktopLifecycle(Mutex<Frontend>);

pub(crate) fn activate(app: &AppHandle) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Err(error) = show_or_create(&handle) {
            log::error!("[desktop] frontend activation failed: {error}");
        }
    }) {
        log::error!("[desktop] could not schedule frontend activation: {error}");
    }
}

pub(crate) fn show_or_create(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    show_or_create_with(app, create)
}

fn show_or_create_with(
    app: &AppHandle,
    create_window: impl FnOnce(
        &AppHandle,
    ) -> Result<
        codeterm_client_shell::app_runtime::WebviewWindow,
        Box<dyn std::error::Error>,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    if crate::crash::shutdown::is_shutting_down() {
        return Ok(());
    }
    let Some(owner) = app.try_state::<DesktopLifecycle>() else {
        log::debug!("[desktop] activation deferred to initial frontend setup");
        return Ok(());
    };
    let action = owner.0.lock().activate();
    match action {
        Activation::Wait => Ok(()),
        Activation::Raise => {
            let window = app
                .get_webview_window("main")
                .ok_or("Main frontend is awaiting its destruction event")?;
            raise(&window)
        }
        Activation::Create => {
            let result = create_window(app);
            app.state::<DesktopLifecycle>()
                .0
                .lock()
                .created(result.is_ok());
            result.map(|_| ())
        }
    }
}

fn raise(
    window: &codeterm_client_shell::app_runtime::WebviewWindow,
) -> Result<(), Box<dyn std::error::Error>> {
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
}

fn create(
    app: &AppHandle,
) -> Result<codeterm_client_shell::app_runtime::WebviewWindow, Box<dyn std::error::Error>> {
    let window = super::frontend_shell::create_window(app)?;
    super::window::setup_window_styling(&window);
    #[cfg(windows)]
    if let Err(error) = crate::native_resize::install(&window) {
        if let Err(cleanup) = window.destroy() {
            log::error!("[desktop] failed frontend cleanup: {cleanup}");
        }
        return Err(error);
    }
    crate::webview_watchdog::spawn(window.clone());
    log::debug!("[desktop] main frontend created");
    Ok(window)
}

pub(crate) fn close_requested(window: &Window) -> bool {
    let close = window
        .app_handle()
        .state::<DesktopLifecycle>()
        .0
        .lock()
        .close();
    if close {
        super::frontend_shell::window_destroying(window.app_handle());
        log::debug!("[desktop] main frontend closing; daemon retained");
    }
    close
}

pub(crate) fn destroyed(app: &AppHandle) {
    super::frontend_shell::window_destroying(app);
    let reopen = app.state::<DesktopLifecycle>().0.lock().destroyed();
    log::debug!("[desktop] main frontend destroyed; reopen={reopen}");
    if reopen {
        activate(app);
    }
}

pub(crate) fn retain_daemon(app: &AppHandle, exit_code: Option<i32>) -> bool {
    if model::retain_daemon(exit_code, crate::crash::shutdown::is_shutting_down()) {
        return true;
    }
    *app.state::<DesktopLifecycle>().0.lock() = Frontend::ShuttingDown;
    false
}

#[cfg(test)]
mod tests;
