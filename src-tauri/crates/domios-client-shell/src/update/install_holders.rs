#[cfg(any(test, windows))]
mod model;
#[cfg(windows)]
mod windows;

pub(super) async fn release_before_installer() -> Result<(), String> {
    #[cfg(windows)]
    return tokio::task::spawn_blocking(|| windows::inspect(true))
        .await
        .map_err(|error| format!("install-holder release task failed: {error}"))?;
    #[cfg(not(windows))]
    Ok(())
}
