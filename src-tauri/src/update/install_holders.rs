#[cfg(any(windows, test))]
mod model;
#[cfg(windows)]
mod windows;

pub(super) async fn check_before_shutdown(
    app: &crate::app_runtime::AppHandle,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use tauri::Manager;
        let svc = app
            .try_state::<crate::service::ServiceState>()
            .ok_or("ServiceState not registered")?
            .inner()
            .clone();
        return tokio::task::spawn_blocking(move || windows::inspect(false, Some(&svc)))
            .await
            .map_err(|error| format!("install-holder discovery task failed: {error}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}

pub(super) async fn release_before_installer() -> Result<(), String> {
    #[cfg(windows)]
    return tokio::task::spawn_blocking(|| windows::inspect(true, None))
        .await
        .map_err(|error| format!("install-holder release task failed: {error}"))?;
    #[cfg(not(windows))]
    Ok(())
}
