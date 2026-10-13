//! `notify_unseen` — the honest condition: a report only when every surface is
//! shut, and nothing at all on the ordinary case.

use super::*;
use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::NotifyDestination;

/// The shipped defaults: the bell shows, the desktop surface is off, no channel
/// is opted in. This is what an install looks like before anyone touches it.
fn shipped() -> NotificationConfig {
    NotificationConfig::default()
}

fn desktop_opted_in() -> NotificationConfig {
    let mut cfg = NotificationConfig {
        desktop: true,
        ..NotificationConfig::default()
    };
    for row in &mut cfg.agent_levels {
        row.set(NotifyDestination::Desktop, true);
    }
    cfg
}

fn desktop_policy(cfg: &NotificationConfig) -> DesktopPolicy {
    DesktopPolicy::from_config(cfg)
}

fn reach<'a>(
    cfg: &NotificationConfig,
    policy: &'a DesktopPolicy,
    declared: Option<NotifyLevel>,
    push: PushReach,
) -> Reach<'a> {
    Reach {
        show_bell: cfg.show_bell,
        declared,
        desktop: policy,
        push,
    }
}

fn surfaces(unseen: &Unseen) -> Vec<&'static str> {
    unseen.closed.iter().map(|closed| closed.surface).collect()
}

#[test]
fn notify_unseen_is_absent_on_the_shipped_install() {
    let cfg = shipped();
    let policy = desktop_policy(&cfg);
    for declared in [
        None,
        Some(NotifyLevel::Verbose),
        Some(NotifyLevel::ActionNeeded),
        Some(NotifyLevel::Failure),
    ] {
        assert!(
            unseen(reach(&cfg, &policy, declared, PushReach::default())).is_none(),
            "the bell shows on a shipped install, so nothing is unseen ({declared:?})"
        );
    }
}

#[test]
fn notify_unseen_is_absent_when_a_level_merely_interrupts_nowhere() {
    // Every interrupting surface closed while the bell still shows IS the
    // shipped shape. Reporting here would fire on the normal case.
    let cfg = shipped();
    assert!(cfg.show_bell, "the shipped install shows the bell");
    let policy = desktop_policy(&cfg);
    assert!(
        unseen(reach(
            &cfg,
            &policy,
            Some(NotifyLevel::Failure),
            PushReach::default()
        ))
        .is_none(),
        "a level that interrupts nowhere is still seen on the bell"
    );
}

#[test]
fn notify_unseen_reports_every_shut_surface_when_the_bell_is_hidden() {
    let cfg = NotificationConfig {
        show_bell: false,
        ..shipped()
    };
    let policy = desktop_policy(&cfg);
    let report = unseen(reach(
        &cfg,
        &policy,
        Some(NotifyLevel::ActionNeeded),
        PushReach::default(),
    ))
    .expect("a hidden bell with no toast and no channel surfaces to nobody");
    assert_eq!(
        surfaces(&report),
        vec![BELL, DESKTOP, PUSH],
        "every surface is named, so the caller reads why rather than a bare warning"
    );
    assert!(
        report
            .closed
            .iter()
            .all(|closed| !closed.reason.trim().is_empty()),
        "each shut surface carries its own reason"
    );
}

#[test]
fn notify_unseen_is_absent_when_the_desktop_toast_still_fires() {
    let cfg = NotificationConfig {
        show_bell: false,
        ..desktop_opted_in()
    };
    let policy = desktop_policy(&cfg);
    assert!(
        unseen(reach(
            &cfg,
            &policy,
            Some(NotifyLevel::Failure),
            PushReach::default()
        ))
        .is_none(),
        "a hidden bell is not unseen while the desktop toast still interrupts"
    );
}

#[test]
fn notify_unseen_is_absent_when_a_push_channel_carries_it() {
    let cfg = NotificationConfig {
        show_bell: false,
        ..shipped()
    };
    let policy = desktop_policy(&cfg);
    assert!(
        unseen(reach(
            &cfg,
            &policy,
            Some(NotifyLevel::Failure),
            PushReach {
                carrying: 1,
                refused: Vec::new(),
            }
        ))
        .is_none(),
        "a channel that will carry it means somebody is alerted"
    );
}

#[test]
fn notify_unseen_names_a_refusing_channel_rather_than_calling_it_unrouted() {
    let cfg = NotificationConfig {
        show_bell: false,
        ..shipped()
    };
    let policy = desktop_policy(&cfg);
    let report = unseen(reach(
        &cfg,
        &policy,
        Some(NotifyLevel::Failure),
        PushReach {
            carrying: 0,
            refused: vec!["telegram".to_string()],
        },
    ))
    .expect("a routed channel that cannot carry it still surfaces to nobody");
    let push = report
        .closed
        .iter()
        .find(|closed| closed.surface == PUSH)
        .expect("the push surface is reported");
    assert!(
        push.reason.contains("telegram"),
        "the reason names the channel that refused, got {}",
        push.reason
    );
}

#[test]
fn notify_unseen_distinguishes_an_unlevelled_record_from_a_closed_level() {
    let cfg = NotificationConfig {
        show_bell: false,
        ..shipped()
    };
    let policy = desktop_policy(&cfg);
    let bare = unseen(reach(&cfg, &policy, None, PushReach::default())).expect("nothing surfaces");
    let levelled = unseen(reach(
        &cfg,
        &policy,
        Some(NotifyLevel::Failure),
        PushReach::default(),
    ))
    .expect("nothing surfaces");
    assert_ne!(
        bare.closed, levelled.closed,
        "naming no level and naming a level nobody routed are different reasons"
    );
}
