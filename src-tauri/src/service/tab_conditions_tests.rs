use codeterm_types::pty::{BootStage, PendingPrompt, PromptIdentity, PromptKind, PromptOption};

use super::{condition_owner, ConditionOwner, ConditionRoute};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabAudience;
use crate::notification::dedupe::TabCondition;
use crate::service::test_fixture::ServiceFixture;

fn prompt(label: &str) -> PendingPrompt {
    PendingPrompt {
        question: "opaque question".into(),
        options: vec![PromptOption {
            label: label.into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

/// Two tabs in one workspace: an owner and the tab under test.
fn two_tabs() -> (MultiplexerState, String, String) {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).expect("workspace");
    let child = mux.create_tab(&workspace).expect("second tab");
    (mux, owner, child)
}

fn records_for(fixture: &ServiceFixture, tab_id: &str) -> Vec<(String, bool, bool)> {
    fixture
        .service
        .notif
        .lock()
        .list(false)
        .into_iter()
        .filter(|record| record.tab_id.as_deref() == Some(tab_id))
        .map(|record| (record.id.clone(), record.read, record.resolved))
        .collect()
}

fn open_records_for(fixture: &ServiceFixture, tab_id: &str) -> Vec<String> {
    records_for(fixture, tab_id)
        .into_iter()
        .filter(|(_, _, resolved)| !resolved)
        .map(|(id, _, _)| id)
        .collect()
}

#[test]
fn only_a_visible_user_audience_tab_is_owned_by_the_user() {
    let (mut mux, owner, child) = two_tabs();
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::User),
        "an undeclared visible tab keeps the user as its owner"
    );

    mux.get_tab_mut(&child).unwrap().audience = TabAudience::Automation;
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::Reporter(None)),
        "an automation tab with no report target is owned by nobody"
    );

    mux.get_tab_mut(&child).unwrap().parent_tab_id = Some(owner.clone());
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::Reporter(Some(owner.clone())))
    );

    let tab = mux.get_tab_mut(&child).unwrap();
    tab.audience = TabAudience::User;
    tab.hidden = true;
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::Reporter(Some(owner))),
        "a hidden tab is never user-facing, whatever its declared audience"
    );
    assert_eq!(condition_owner(&mux, "missing"), None);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn non_user_facing_tab_routes_prompt_to_parent() {
    let (mut mux, owner, child) = two_tabs();
    {
        let tab = mux.get_tab_mut(&child).unwrap();
        tab.audience = TabAudience::Automation;
        tab.parent_tab_id = Some(owner.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("opaque-a");

    let first = fixture
        .service
        .notify_pending_prompt(&child, &asked, PromptKind::Provider);
    let again = fixture
        .service
        .notify_pending_prompt(&child, &asked, PromptKind::Provider);

    assert_eq!(first, Some(ConditionRoute::Reported { to: owner.clone() }));
    assert_eq!(
        again,
        Some(ConditionRoute::AlreadyReported),
        "one prompt reaches its owner once"
    );
    assert!(
        records_for(&fixture, &child).is_empty(),
        "a non-user-facing tab never opens a user notification"
    );

    let identity = PromptIdentity::new(PromptKind::Provider, &asked);
    fixture
        .service
        .resolve_prompt_notifications(&child, &identity);
    let next =
        fixture
            .service
            .notify_pending_prompt(&child, &prompt("opaque-b"), PromptKind::Provider);
    assert_eq!(
        next,
        Some(ConditionRoute::Reported { to: owner }),
        "a cleared prompt releases the latch, so the next prompt is news"
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn hidden_tab_without_report_target_stays_off_the_user_surfaces() {
    let (mut mux, _owner, child) = two_tabs();
    mux.get_tab_mut(&child).unwrap().hidden = true;
    let fixture = ServiceFixture::new(mux);

    let route =
        fixture
            .service
            .notify_pending_prompt(&child, &prompt("opaque"), PromptKind::Provider);
    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");

    assert_eq!(route, Some(ConditionRoute::Unowned));
    assert!(records_for(&fixture, &child).is_empty());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn user_facing_prompt_is_one_notice_until_it_resolves() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("opaque");

    let first = fixture
        .service
        .notify_pending_prompt(&child, &asked, PromptKind::Provider);
    let again = fixture
        .service
        .notify_pending_prompt(&child, &asked, PromptKind::Provider);

    let Some(ConditionRoute::Notified(id)) = first else {
        panic!("a user-facing prompt opens a notification: {first:?}");
    };
    assert_eq!(again, Some(ConditionRoute::Notified(id.clone())));
    assert_eq!(open_records_for(&fixture, &child), vec![id.clone()]);

    fixture
        .service
        .resolve_prompt_notifications(&child, &PromptIdentity::new(PromptKind::Provider, &asked));
    assert!(open_records_for(&fixture, &child).is_empty());
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn restart_resolves_exit_notification() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");
    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");
    let open = open_records_for(&fixture, &child);
    assert_eq!(open.len(), 1, "one exit is one notice: {open:?}");
    {
        let store = fixture.service.notif.lock();
        let record = store.notification(&open[0]).unwrap();
        assert_eq!(record.condition, Some(TabCondition::AgentExited));
        assert_eq!(record.sticky, Some(true));
    }

    fixture
        .service
        .emit_boot_stage(&child, BootStage::AgentReady, None);

    assert!(
        open_records_for(&fixture, &child).is_empty(),
        "a restarted agent reaching readiness ends the exit condition"
    );
    let resolved = records_for(&fixture, &child);
    assert!(resolved
        .iter()
        .all(|(_, read, resolved)| *read && *resolved));

    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-2");
    assert_eq!(
        open_records_for(&fixture, &child).len(),
        1,
        "a later exit is news again"
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn readiness_keeps_a_prompt_that_is_still_showing() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("opaque");
    fixture
        .service
        .notify_pending_prompt(&child, &asked, PromptKind::Provider);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&child).unwrap();
        tab.pending_prompt = Some(asked);
        tab.pending_prompt_kind = Some(PromptKind::Provider);
    }

    fixture
        .service
        .emit_boot_stage(&child, BootStage::AgentReady, None);

    assert_eq!(open_records_for(&fixture, &child).len(), 1);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn tab_close_resolves_exit_and_prompt_notifications() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");
    fixture
        .service
        .notify_pending_prompt(&child, &prompt("opaque"), PromptKind::Provider);
    assert_eq!(open_records_for(&fixture, &child).len(), 2);

    fixture.service.close_tab(&child).unwrap();

    assert!(open_records_for(&fixture, &child).is_empty());
}
