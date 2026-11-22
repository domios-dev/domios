#[cfg(any(test, windows))]
mod model;
#[cfg(windows)]
mod windows;

pub(super) struct PreparedHolders {
    #[cfg(windows)]
    owned_server: windows::OwnedServerHolders,
    #[cfg(windows)]
    tabs: Vec<codeterm_update::holders::HolderTab>,
    initiating_tab: Option<String>,
}

impl PreparedHolders {
    pub(super) fn shutdown_reservation_request(&self) -> codeterm_update::holders::ReserveHolders {
        codeterm_update::holders::ReserveHolders {
            #[cfg(windows)]
            tabs: self.tabs.clone(),
            #[cfg(not(windows))]
            tabs: Vec::new(),
            initiating_tab: self.initiating_tab.clone(),
        }
    }

    pub(super) async fn confirm_exited(self) -> Result<(), String> {
        #[cfg(windows)]
        return tokio::task::spawn_blocking(move || {
            self.owned_server.confirm_exited()?;
            windows::release_with_tabs(&self.tabs)
        })
        .await
        .map_err(|error| format!("install-holder exit task failed: {error}"))?;
        #[cfg(not(windows))]
        Ok(())
    }
}

pub(super) async fn preflight(
    _server: crate::application_host::AttachedServerIdentity,
    _stop: &dyn crate::application_host::ServerStop,
    _initiating_tab: Option<&str>,
) -> Result<PreparedHolders, String> {
    #[cfg(windows)]
    {
        let processes = tokio::task::spawn_blocking(windows::holder_processes)
            .await
            .map_err(|e| format!("install-holder discovery task failed: {e}"))??;
        let tabs = _stop.describe_install_holders(processes).await?;
        let initiating_tab = _initiating_tab.map(str::to_owned);
        return tokio::task::spawn_blocking(move || {
            windows::preflight_with_tabs(_server, &tabs).map(|owned_server| PreparedHolders {
                owned_server,
                tabs,
                initiating_tab,
            })
        })
        .await
        .map_err(|error| format!("install-holder preflight task failed: {error}"))?;
    }
    #[cfg(not(windows))]
    Ok(PreparedHolders {
        initiating_tab: _initiating_tab.map(str::to_owned),
    })
}
