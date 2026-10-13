use crate::multiplexer::types::{ShellActivity, ShellPurpose, Tab};
use tauri::Manager;

pub(super) static SPAWN_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

pub(super) fn configured_limits(
    app: &crate::app_runtime::AppHandle,
) -> crate::config::schema::UtilityTabLimits {
    app.try_state::<crate::config::ConfigState>()
        .map(|config| config.lock().utility_tabs.max_per_team.clone())
        .unwrap_or_else(|| crate::config::load_config().utility_tabs.max_per_team)
}

pub(super) fn normalize_icon(icon: Option<String>) -> Result<Option<String>, String> {
    let Some(icon) = icon else { return Ok(None) };
    let name = icon.trim().to_ascii_lowercase();
    let names: Vec<String> =
        serde_json::from_str(include_str!("../../../src/lib/lucideIconNames.json"))
            .map_err(|error| error.to_string())?;
    if names.binary_search(&name).is_err() {
        return Err(format!("Unknown Lucide icon '{icon}'; use a kebab-case name such as bug, hammer or flask-conical"));
    }
    Ok(Some(name))
}

pub(super) fn limit_for(
    limits: &crate::config::schema::UtilityTabLimits,
    purpose: ShellPurpose,
) -> Option<usize> {
    match purpose {
        ShellPurpose::Build => limits.build,
        ShellPurpose::Dev => limits.dev,
        ShellPurpose::Qa => limits.qa,
        ShellPurpose::Deploy => limits.deploy,
        ShellPurpose::Logs => limits.logs,
        ShellPurpose::Db => limits.db,
        ShellPurpose::Shell => limits.shell,
    }
}

pub(super) fn select_slot<'a>(
    tabs: impl Iterator<Item = &'a Tab>,
    team: &str,
    purpose: ShellPurpose,
    limit: Option<usize>,
) -> Result<Option<String>, String> {
    let Some(limit) = limit else { return Ok(None) };
    let tabs: Vec<_> = tabs
        .filter(|tab| {
            tab.is_utility_shell()
                && !tab.hidden
                && tab.team_id.as_deref() == Some(team)
                && tab.purpose == purpose
        })
        .collect();
    if tabs.len() < limit {
        return Ok(None);
    }
    tabs.into_iter()
        .filter(|tab| tab.shell_command_state.activity == ShellActivity::Idle && !tab.utility_command_pending)
        .min_by(|left, right| left.utility_created_at.cmp(&right.utility_created_at).then_with(|| left.id.cmp(&right.id)))
        .map(|tab| Some(tab.id.clone()))
        .ok_or_else(|| format!("utility_tab_limit: Team {team} reached its {purpose:?} utility tab limit ({limit}); no idle tab is available. Wait for a running tab to finish or close one"))
}

#[cfg(test)]
mod tests;
