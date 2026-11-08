use super::engine::Session;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::RuntimeSettings;
use serde_json::Value;

pub const FULL_ACCESS_UNAVAILABLE: &str = "full_access_unavailable";

fn string_setting<'a>(provider_settings: &'a Value, key: &str) -> Option<&'a str> {
    provider_settings.get(key).and_then(Value::as_str)
}

/// Why a session that loaded did not reach the full-access mode its launch declared.
///
/// A different mode the owner chose replaces the declared one, so it is never a failure.
pub(super) fn missing_full_access(
    provider_settings: &Value,
    loaded: &RuntimeSettings,
) -> Option<String> {
    let declared = string_setting(provider_settings, "fullAccessMode")?;
    if string_setting(
        provider_settings,
        super::permission_policy::SESSION_MODE_KEY,
    )
    .is_some_and(|chosen| chosen != declared)
        || loaded.mode.as_deref() == Some(declared)
    {
        return None;
    }
    let advertised = loaded
        .modes
        .iter()
        .flatten()
        .map(|mode| mode.id.as_str())
        .collect::<Vec<_>>();
    let current = loaded.mode.as_deref().unwrap_or("unknown");
    Some(if advertised.contains(&declared) {
        format!("{FULL_ACCESS_UNAVAILABLE}: agent refused declared mode {declared}; running in {current} under its own sandbox")
    } else {
        format!(
            "{FULL_ACCESS_UNAVAILABLE}: declared mode {declared} not advertised ({}); running in {current} under its own sandbox",
            if advertised.is_empty() {
                "no modes".to_owned()
            } else {
                advertised.join(", ")
            }
        )
    })
}

/// Settles what a load reports: a declared full-access mode the session did not reach is named on
/// the tab, in the log and to the tab's parent, because the agent then works inside a sandbox.
pub(super) fn finish_load(svc: &ServiceContext, session: &Session, load_notice: Option<String>) {
    let context = session.context.lock().clone();
    let failure = missing_full_access(&context.provider_settings, &context.settings);
    {
        let mut state = session.state.lock();
        state.access_failure = failure.clone();
        let notice = [load_notice, failure.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        state.finish_load_notice((!notice.is_empty()).then(|| notice.join("\n")));
    }
    let Some(reason) = failure else {
        return;
    };
    log::error!(
        "[runtime-launch] tab={} provider={} {reason}",
        context.tab_id,
        session.provider
    );
    let owner = svc.clone();
    crate::async_runtime::spawn_blocking(move || {
        owner.report_full_access_unavailable(&context.tab_id, &reason);
    });
}

/// The owner entering the declared mode live ends the failure; any other mode they pick leaves it.
pub(super) fn observe_confirmed(session: &Session, confirmed: &RuntimeSettings) {
    let reached = string_setting(&session.context.lock().provider_settings, "fullAccessMode")
        .is_some_and(|declared| confirmed.mode.as_deref() == Some(declared));
    if !reached {
        return;
    }
    let mut state = session.state.lock();
    let Some(failure) = state.access_failure.take() else {
        return;
    };
    state.notice = state
        .notice
        .take()
        .map(|notice| {
            notice
                .lines()
                .filter(|line| *line != failure)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|notice| !notice.is_empty());
}

#[cfg(test)]
#[path = "full_access/tests.rs"]
mod tests;
