use super::*;

fn reveal_with(
    visible: Result<bool, String>,
    show: impl FnOnce() -> Result<(), String>,
    dismiss_starting: impl FnOnce(),
) -> Result<(), String> {
    if !visible? {
        show()?;
    }
    dismiss_starting();
    Ok(())
}

pub(crate) fn reveal_main(app: &codeterm_client_shell::app_runtime::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let shown = reveal_with(
        window.is_visible().map_err(|error| error.to_string()),
        || {
            window
                .show()
                .and_then(|()| window.unminimize())
                .and_then(|()| window.set_focus())
                .map_err(|error| error.to_string())
        },
        || crate::startup::starting_window::dismiss(app),
    );
    if let Err(error) = shown {
        log::debug!("[frontend-shell] window reveal failed: {error}");
    }
}

pub(super) fn show_unavailable(window: &WebviewWindow) {
    if let Some(starting) = window.app_handle().get_webview_window("starting") {
        show_start_failure(
            &starting,
            "The Domios interface could not be loaded. Retry to reconnect.",
        );
        return;
    }
    reveal_main(window.app_handle());
    // Preserve the recovery document across repeated disconnection events.
    let on_recovery_surface = ON_RECOVERY_SURFACE.swap(true, Ordering::AcqRel);
    if !ON_START_FAILURE.swap(false, Ordering::AcqRel) && on_recovery_surface {
        return;
    }
    match unavailable_url()
        .map_err(|error| error.to_string())
        .and_then(|url| window.navigate(url).map_err(|error| error.to_string()))
    {
        Ok(()) => {
            log::debug!("[frontend-shell] native recovery surface selected");
        }
        Err(error) => {
            ON_RECOVERY_SURFACE.store(false, Ordering::Release);
            log::error!("[frontend-shell] recovery surface failed: {error}");
        }
    }
}

/// A fragment-only change keeps the document, so a repeated refusal just updates its reason.
pub(crate) fn show_start_failure(window: &WebviewWindow, reason: &str) {
    if window.label() == "main" {
        reveal_main(window.app_handle());
    }
    ON_RECOVERY_SURFACE.store(true, Ordering::Release);
    ON_START_FAILURE.store(true, Ordering::Release);
    match start_failure_url(Some(reason))
        .map_err(|error| error.to_string())
        .and_then(|url| window.navigate(url).map_err(|error| error.to_string()))
    {
        Ok(()) => log::debug!("[frontend-shell] start failure surface selected"),
        Err(error) => {
            ON_START_FAILURE.store(false, Ordering::Release);
            ON_RECOVERY_SURFACE.store(false, Ordering::Release);
            log::error!("[frontend-shell] start failure surface failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
