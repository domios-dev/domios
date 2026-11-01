#[cfg(any(test, windows))]
mod model;
#[cfg(windows)]
mod windows;

pub(super) struct PreparedHolders {
    #[cfg(windows)]
    owned_server: windows::OwnedServerHolders,
}

impl PreparedHolders {
    pub(super) async fn confirm_exited(self) -> Result<(), String> {
        #[cfg(windows)]
        return tokio::task::spawn_blocking(move || self.owned_server.confirm_exited())
            .await
            .map_err(|error| format!("install-holder exit task failed: {error}"))?;
        #[cfg(not(windows))]
        Ok(())
    }
}

pub(super) async fn release_before_installer() -> Result<(), String> {
    #[cfg(windows)]
    return tokio::task::spawn_blocking(|| windows::inspect(true))
        .await
        .map_err(|error| format!("install-holder release task failed: {error}"))?;
    #[cfg(not(windows))]
    Ok(())
}

pub(super) async fn preflight(
    _server: crate::application_host::AttachedServerIdentity,
) -> Result<PreparedHolders, String> {
    #[cfg(windows)]
    return tokio::task::spawn_blocking(move || {
        windows::preflight(_server).map(|owned_server| PreparedHolders { owned_server })
    })
    .await
    .map_err(|error| format!("install-holder preflight task failed: {error}"))?;
    #[cfg(not(windows))]
    Ok(PreparedHolders {})
}
