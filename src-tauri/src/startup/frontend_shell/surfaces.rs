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

pub(super) fn show_unavailable(window: &WebviewWindow, code: FailureCode, reason: &str) {
    let detail = code.record(reason);
    show_unavailable_recorded(window, &detail);
}

pub(super) fn show_unavailable_recorded(window: &WebviewWindow, detail: &str) {
    if let Some(starting) = window.app_handle().get_webview_window("starting") {
        show_start_failure_recorded(&starting, detail);
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
            show_navigation_failure(window, FailureCode::RecoveryNavigationFailed, &error);
            log::error!("[frontend-shell] recovery surface failed: {error}");
        }
    }
}

/// A fragment-only change keeps the document, so a repeated refusal just updates its reason.
pub(crate) fn show_start_failure(window: &WebviewWindow, code: FailureCode, reason: &str) {
    let detail = code.record(reason);
    show_start_failure_recorded(window, &detail);
}

fn show_start_failure_recorded(window: &WebviewWindow, detail: &str) {
    if window.label() == "main" {
        reveal_main(window.app_handle());
    }
    ON_RECOVERY_SURFACE.store(true, Ordering::Release);
    ON_START_FAILURE.store(true, Ordering::Release);
    match start_failure_url(Some(detail))
        .map_err(|error| error.to_string())
        .and_then(|url| window.navigate(url).map_err(|error| error.to_string()))
    {
        Ok(()) => log::debug!("[frontend-shell] start failure surface selected"),
        Err(error) => {
            ON_START_FAILURE.store(false, Ordering::Release);
            ON_RECOVERY_SURFACE.store(false, Ordering::Release);
            show_navigation_failure(window, FailureCode::FailureNavigationFailed, &error);
            log::error!("[frontend-shell] start failure surface failed: {error}");
        }
    }
}

fn show_navigation_failure(window: &WebviewWindow, code: FailureCode, reason: &str) {
    let detail = code.record(reason);
    if let Ok(text) = serde_json::to_string(&detail) {
        let _ = window.eval(&format!(
            "(() => {{ let p = document.querySelector('.detail'); if (!p) {{ const d = document.createElement('details'); const s = document.createElement('summary'); s.textContent = 'Details'; d.append(s); p = document.createElement('p'); p.className = 'detail'; d.append(p); (document.querySelector('main') || document.body).append(d); }} p.textContent = {text}; p.hidden = false; p.parentElement.hidden = false; p.parentElement.open = true; }})()"
        ));
    }
}

#[cfg(test)]
mod tests;
