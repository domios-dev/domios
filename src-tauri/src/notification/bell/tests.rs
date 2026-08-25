//! `notification_settings` — the bell leg of the grid, held to the same
//! standard as the desktop gate it sits beside.

use super::*;
use codeterm_types::config::schema::{AgentLevelChoice, NotificationConfig};

fn config_with(rows: Vec<AgentLevelChoice>) -> NotificationConfig {
    NotificationConfig {
        agent_levels: rows,
        ..NotificationConfig::default()
    }
}

fn row(level: NotifyLevel, bell: bool) -> AgentLevelChoice {
    let mut choice = AgentLevelChoice::open(level.as_str(), &[]);
    choice.set(NotifyDestination::Bell, bell);
    choice
}

#[test]
fn notification_settings_shipped_bell_column_admits_every_level() {
    let policy = BellPolicy::from_config(&NotificationConfig::default());
    assert_eq!(
        policy.allowed.len(),
        NotifyLevel::ALL.len(),
        "the shipped grid routes every level to the bell, which is all it routes"
    );
    for level in NotifyLevel::ALL {
        assert!(admit(Some(level), &policy).reaches_bell());
    }
    assert!(
        admit(None, &policy).reaches_bell(),
        "a message that names no level is kept and rings"
    );
}

#[test]
fn notification_settings_bell_column_off_refuses_only_that_level() {
    let policy = BellPolicy::from_config(&config_with(vec![
        row(NotifyLevel::Verbose, false),
        row(NotifyLevel::ActionNeeded, true),
        row(NotifyLevel::Failure, true),
    ]));

    assert_eq!(
        admit(Some(NotifyLevel::Verbose), &policy),
        BellAdmission::LevelDisabled(NotifyLevel::Verbose),
        "the refusal names the level rather than dropping it silently"
    );
    assert!(admit(Some(NotifyLevel::ActionNeeded), &policy).reaches_bell());
    assert!(admit(Some(NotifyLevel::Failure), &policy).reaches_bell());
    assert!(
        admit(None, &policy).reaches_bell(),
        "closing a column never closes the unlevelled route"
    );
}

/// Every column off for every level is a configuration, not an error: the bell
/// is drawn and stays empty.
#[test]
fn notification_settings_every_bell_column_off_leaves_the_surface_drawn_but_empty() {
    let cfg = config_with(
        NotifyLevel::ALL
            .into_iter()
            .map(|level| row(level, false))
            .collect(),
    );
    let policy = BellPolicy::from_config(&cfg);

    assert!(policy.allowed.is_empty());
    for level in NotifyLevel::ALL {
        assert!(!admit(Some(level), &policy).reaches_bell());
    }
    assert!(
        cfg.show_bell,
        "an empty bell is still a bell: the surface is drawn"
    );
}

/// The distinction the master switch exists to make: hidden surface versus
/// unreachable column. Neither overrides nor disables the other.
#[test]
fn notification_settings_master_switch_hides_the_surface_without_touching_routing() {
    let mut hidden = NotificationConfig {
        show_bell: false,
        ..NotificationConfig::default()
    };
    assert!(!hidden.show_bell, "the indicator is not drawn");
    let policy = BellPolicy::from_config(&hidden);
    for level in NotifyLevel::ALL {
        assert!(
            admit(Some(level), &policy).reaches_bell(),
            "hiding the surface routes nothing away from it, so the log stays complete"
        );
    }

    // Composition is orthogonal: hiding the surface leaves closed columns closed.
    hidden.agent_levels = NotifyLevel::ALL
        .into_iter()
        .map(|level| row(level, false))
        .collect();
    let composed = BellPolicy::from_config(&hidden);
    assert!(!hidden.show_bell);
    assert!(composed.allowed.is_empty());
}

/// One unreadable row costs that row only — never the whole read.
#[test]
fn notification_settings_unparseable_bell_row_is_dropped_and_the_rest_still_read() {
    let policy = BellPolicy::from_config(&config_with(vec![
        row(NotifyLevel::Verbose, true),
        AgentLevelChoice::open("milestone", &[]),
        AgentLevelChoice::open("", &[]),
        row(NotifyLevel::Failure, true),
    ]));

    assert_eq!(policy.allowed, [NotifyLevel::Verbose, NotifyLevel::Failure]);
    assert!(!admit(Some(NotifyLevel::ActionNeeded), &policy).reaches_bell());
}

#[test]
fn notification_settings_open_policy_is_the_pre_grid_behaviour() {
    let policy = BellPolicy::open();
    for level in NotifyLevel::ALL {
        assert!(policy.allows(level));
    }
    assert!(admit(None, &policy).reaches_bell());
}
