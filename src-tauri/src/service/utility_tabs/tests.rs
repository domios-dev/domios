use super::*;
use crate::multiplexer::state::MultiplexerState;

fn tab(id: &str, age: u64, activity: ShellActivity) -> Tab {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&first).unwrap();
    let shell = mux.create_tab(&workspace).unwrap();
    let mut tab = mux.get_tab(&shell).unwrap().clone();
    tab.id = id.into();
    tab.team_id = Some("team".into());
    tab.purpose = ShellPurpose::Build;
    tab.utility_created_at = age;
    tab.shell_command_state.activity = activity;
    assert!(tab.is_utility_shell());
    tab
}

#[test]
fn utility_slots_unlimited_and_below_limit_create() {
    let tabs = [tab("busy", 1, ShellActivity::Running)];
    assert_eq!(
        select_slot(tabs.iter(), "team", ShellPurpose::Build, None),
        Ok(None)
    );
    assert_eq!(
        select_slot(tabs.iter(), "team", ShellPurpose::Build, Some(2)),
        Ok(None)
    );
}

#[test]
fn utility_slots_reuse_oldest_idle_and_refuse_busy_unknown_or_reserved() {
    let tabs = [
        tab("old", 1, ShellActivity::Running),
        tab("idle", 2, ShellActivity::Idle),
        tab("new", 3, ShellActivity::Idle),
    ];
    assert_eq!(
        select_slot(tabs.iter(), "team", ShellPurpose::Build, Some(2)),
        Ok(Some("idle".into()))
    );
    for activity in [ShellActivity::Running, ShellActivity::Unknown] {
        let tabs = [tab("busy", 1, activity)];
        assert!(
            select_slot(tabs.iter(), "team", ShellPurpose::Build, Some(1))
                .unwrap_err()
                .starts_with("utility_tab_limit:")
        );
    }
    let mut reserved = tab("reserved", 1, ShellActivity::Idle);
    reserved.utility_command_pending = true;
    assert!(select_slot(
        [&reserved].into_iter(),
        "team",
        ShellPurpose::Build,
        Some(1)
    )
    .is_err());
    assert!(select_slot(std::iter::empty(), "team", ShellPurpose::Build, Some(0)).is_err());
}

#[test]
fn utility_slots_exclude_other_teams_purposes_agents_and_hidden_tabs() {
    let mut tabs = vec![
        tab("other-team", 1, ShellActivity::Idle),
        tab("qa", 2, ShellActivity::Idle),
        tab("agent", 3, ShellActivity::Idle),
        tab("hidden", 4, ShellActivity::Idle),
    ];
    tabs[0].team_id = Some("elsewhere".into());
    tabs[1].purpose = ShellPurpose::Qa;
    tabs[2].roleless_tracked = true;
    tabs[3].hidden = true;
    assert_eq!(
        select_slot(tabs.iter(), "team", ShellPurpose::Build, Some(1)),
        Ok(None)
    );
}

#[test]
fn utility_icons_validate_before_creation() {
    for name in ["bug", "hammer", "flask-conical", "a-arrow-down"] {
        assert_eq!(
            normalize_icon(Some(name.into())).unwrap(),
            Some(name.into())
        );
    }
    assert_eq!(normalize_icon(None).unwrap(), None);
    assert!(normalize_icon(Some("missing-icon".into()))
        .unwrap_err()
        .contains("kebab-case"));
}

#[test]
fn utility_limits_config_defaults_and_per_purpose_values() {
    let default = crate::config::schema::AppConfig::default();
    for purpose in ShellPurpose::ALL {
        assert_eq!(limit_for(&default.utility_tabs.max_per_team, purpose), None);
    }
    let config: crate::config::schema::AppConfig = serde_json::from_value(serde_json::json!({
        "utility_tabs": {"max_per_team": {"build": 2, "qa": 1, "deploy": 1, "logs": 3, "db": 4}}
    }))
    .unwrap();
    assert_eq!(
        limit_for(&config.utility_tabs.max_per_team, ShellPurpose::Build),
        Some(2)
    );
    assert_eq!(
        limit_for(&config.utility_tabs.max_per_team, ShellPurpose::Qa),
        Some(1)
    );
    assert_eq!(
        limit_for(&config.utility_tabs.max_per_team, ShellPurpose::Dev),
        None
    );
    for (purpose, limit) in [
        (ShellPurpose::Deploy, 1),
        (ShellPurpose::Logs, 3),
        (ShellPurpose::Db, 4),
    ] {
        assert_eq!(
            limit_for(&config.utility_tabs.max_per_team, purpose),
            Some(limit)
        );
    }
}
