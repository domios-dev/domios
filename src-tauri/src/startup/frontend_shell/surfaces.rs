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
    show_unavailable_recorded(window, code, &detail, reason);
}

pub(super) fn show_unavailable_recorded(
    window: &WebviewWindow,
    code: FailureCode,
    detail: &str,
    reason: &str,
) {
    if reconnect::recover_before_failure(window.app_handle(), code, reason) {
        return;
    }
    let target = window
        .app_handle()
        .get_webview_window("starting")
        .unwrap_or_else(|| window.clone());
    show_start_failure_recorded(&target, detail);
}

/// A fragment-only change keeps the document, so a repeated refusal just updates its reason.
pub(crate) fn show_start_failure(window: &WebviewWindow, code: FailureCode, reason: &str) {
    let detail = code.record(reason);
    if !reconnect::recover_before_failure(window.app_handle(), code, reason) {
        show_start_failure_recorded(window, &detail);
    }
}

pub(super) fn show_start_failure_recorded(window: &WebviewWindow, detail: &str) {
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

pub(super) fn show_reconnecting(
    app: &codeterm_client_shell::app_runtime::AppHandle,
) -> Result<(), String> {
    ON_RECOVERY_SURFACE.store(true, Ordering::Release);
    ON_START_FAILURE.store(false, Ordering::Release);
    if app.get_webview_window("starting").is_none() {
        crate::startup::starting_window::show(app).map_err(|error| error.to_string())?;
    }
    let starting = app
        .get_webview_window("starting")
        .ok_or("Starting window is unavailable")?;
    starting
        .navigate(unavailable_url().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
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
