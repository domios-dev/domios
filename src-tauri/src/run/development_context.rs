use codeterm_types::server_bootstrap::InstanceSelector;
use std::io;
use tauri::utils::config::Config;

pub(super) fn configure(
    config: &mut Config,
    debug: bool,
    selector: &InstanceSelector,
    environment: impl Fn(&str) -> Option<String>,
) -> io::Result<()> {
    if !debug || !matches!(selector, InstanceSelector::Development { .. }) {
        return Ok(());
    }
    let frontend = codeterm_frontend_shell::development_runtime_frontend(
        true,
        true,
        &environment("CODETERM_VITE_PORT").unwrap_or_default(),
        &environment("CODETERM_DEV_IDENTIFIER").unwrap_or_default(),
    )
    .map_err(io::Error::other)?
    .ok_or_else(|| {
        io::Error::other("Development client requires explicit frontend runtime identity")
    })?;
    let arguments = codeterm_frontend_shell::development_browser_arguments(
        &environment("CODETERM_CDP_PORT").unwrap_or_else(|| "9223".into()),
        &environment("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default(),
    )
    .map_err(io::Error::other)?;
    config.identifier = frontend.identifier;
    config.build.dev_url = Some(frontend.origin);
    for window in &mut config.app.windows {
        window.additional_browser_args = Some(arguments.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
