use crate::agent::registry::AgentRegistry;
use crate::service::ServiceContext;

pub(crate) fn for_tab(
    service: &ServiceContext,
    registry: &AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    launch_cwd: Option<&str>,
) {
    let cwd = {
        let mux = service.mux.read();
        mux.get_tab(tab_id)
            .and_then(|tab| eligible_cwd(tab, launch_cwd).map(str::to_string))
    };
    let Some(cwd) = cwd else {
        return;
    };
    let result = registry.with_provider(provider_id, |provider| provider.pre_trust_directory(&cwd));
    match result {
        Some(Ok(outcome)) => log::info!("[directory-pre-trust] tab={tab_id} outcome={outcome:?}"),
        Some(Err(_)) => log::warn!(
            "[directory-pre-trust] tab={tab_id} capability failed; TUI fallback remains active"
        ),
        None => log::debug!("[directory-pre-trust] tab={tab_id} capability unavailable"),
    }
}

fn eligible_cwd<'a>(
    tab: &'a crate::multiplexer::types::Tab,
    launch_cwd: Option<&str>,
) -> Option<&'a str> {
    authorized_cwd(
        super::directory_trust::directory_trust_check_applies(tab),
        tab.cwd.as_deref(),
        launch_cwd,
    )
}

fn authorized_cwd<'a>(
    authorized: bool,
    tab_cwd: Option<&'a str>,
    launch_cwd: Option<&str>,
) -> Option<&'a str> {
    if !authorized {
        return None;
    }
    let cwd = tab_cwd.filter(|cwd| !cwd.is_empty())?;
    ServiceContext::paths_equal(cwd, launch_cwd.unwrap_or(cwd)).then_some(cwd)
}

#[cfg(test)]
mod tests;
