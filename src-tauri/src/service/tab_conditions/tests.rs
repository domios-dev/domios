use codeterm_types::pty::{BootStage, PendingPrompt, PromptIdentity, PromptKind, PromptOption};
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::Manager;

use super::{condition_owner, ConditionOwner};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabAudience;
use crate::notification::dedupe::TabCondition;
use crate::service::test_fixture::ServiceFixture;

fn prompt(label: &str) -> PendingPrompt {
    PendingPrompt {
        request_id: None,
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

fn seed_prompt_condition(fixture: &ServiceFixture, tab_id: &str) -> String {
    fixture.service.notif.lock().add_ext(
        "opaque condition".into(),
        Some("opaque body".into()),
        crate::notification::store::NotificationSeverity::Warning,
        crate::notification::store::NotificationProvenance {
            tab_id: Some(tab_id.into()),
            ..Default::default()
        },
        crate::notification::store::Ask::about(TabCondition::PendingPrompt, None),
        None,
        None,
    )
}

fn set_pending_prompt(fixture: &ServiceFixture, tab_id: &str, prompt: PendingPrompt) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).expect("tab");
    tab.pending_prompt = Some(prompt);
    tab.pending_prompt_kind = Some(PromptKind::Provider);
}

#[test]
fn parent_prompt_report_body_includes_parsed_options() {
    let mut asked = prompt("Allow once");
    asked.question = "opaque permission question".into();
    asked.options[0].value = "allow_once".into();

    let body = super::parent_prompt_report_body("prompt-tab", &asked);

    assert!(body.contains("opaque permission question"));
    assert!(body.contains("codeterm tab respond <option> --tab prompt-tab"));
    assert!(body.contains("1. Allow once (value: allow_once)"));
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

    mux.get_tab_mut(&child).unwrap().audience = TabAudience::UserOwnedAutomation;
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::User),
        "user-owned automation without a live creator is owed to the user"
    );

    mux.get_tab_mut(&child).unwrap().parent_tab_id = Some(owner.clone());
    mux.get_tab_mut(&child).unwrap().spawned_by_tab_id = Some(owner.clone());
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::Reporter(Some(owner.clone())))
    );

    mux.get_tab_mut(&child).unwrap().audience = TabAudience::Automation;
    assert_eq!(
        condition_owner(&mux, &child),
        Some(ConditionOwner::Reporter(Some(owner.clone()))),
        "automation with a report target stays owned by its reporter"
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
fn non_user_facing_tab_routes_prompt_to_parent() {
    let (mut mux, owner, child) = two_tabs();
    {
        let tab = mux.get_tab_mut(&child).unwrap();
        tab.audience = TabAudience::Automation;
        tab.parent_tab_id = Some(owner.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("opaque-a");
    set_pending_prompt(&fixture, &child, asked.clone());

    let first = fixture
        .service
        .route_pending_prompt(&child, &asked, PromptKind::Provider);
    let again = fixture
        .service
        .route_pending_prompt(&child, &asked, PromptKind::Provider);

    assert_eq!(first, Some(owner.clone()));
    assert_eq!(again, None, "reobserving the prompt stays quiet");
    assert!(
        records_for(&fixture, &child).is_empty(),
        "a routed prompt does not create a notification"
    );

    fixture.service.clear_pending_prompt(&child);
    fixture
        .service
        .resolve_prompt_notifications(&child, &PromptIdentity::new(PromptKind::Provider, &asked));
    let next = prompt("opaque-b");
    set_pending_prompt(&fixture, &child, next.clone());
    assert_eq!(
        fixture
            .service
            .route_pending_prompt(&child, &next, PromptKind::Provider),
        Some(owner)
    );
}

#[test]
fn user_audience_prompt_stays_out_of_bell_and_routes_parsed_options_once() {
    let (mut mux, owner, child) = two_tabs();
    {
        let tab = mux.get_tab_mut(&child).unwrap();
        tab.audience = TabAudience::User;
        tab.parent_tab_id = Some(owner.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut asked = prompt("Trust once");
    asked.question = "opaque trust question".into();
    asked.multi_select = Some(true);
    asked.options.push(PromptOption {
        label: "Trust always".into(),
        value: "trust_always".into(),
    });
    set_pending_prompt(&fixture, &child, asked.clone());

    assert_eq!(
        fixture
            .service
            .route_pending_prompt(&child, &asked, PromptKind::Provider),
        Some(owner.clone())
    );
    assert_eq!(
        fixture
            .service
            .route_pending_prompt(&child, &asked, PromptKind::Provider),
        None
    );

    assert!(records_for(&fixture, &child).is_empty());
    let report = super::parent_prompt_report_body(&child, &asked);
    assert!(report.contains("1. Trust once (value: 1)"));
    assert!(report.contains("2. Trust always (value: trust_always)"));
}

#[test]
fn owner_launched_prompt_stays_out_of_bell_without_parent_route() {
    let (mux, _, tab_id) = two_tabs();
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("Trust once");
    set_pending_prompt(&fixture, &tab_id, asked.clone());

    for _ in 0..2 {
        assert_eq!(
            fixture
                .service
                .route_pending_prompt(&tab_id, &asked, PromptKind::Provider),
            None
        );
    }

    assert!(records_for(&fixture, &tab_id).is_empty());
}

#[test]
fn hidden_tab_without_report_target_stays_off_the_user_surfaces() {
    let (mut mux, _owner, child) = two_tabs();
    mux.get_tab_mut(&child).unwrap().hidden = true;
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("opaque");
    set_pending_prompt(&fixture, &child, asked.clone());

    let routed = fixture
        .service
        .route_pending_prompt(&child, &asked, PromptKind::Provider);
    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");

    assert_eq!(routed, None);
    assert!(records_for(&fixture, &child).is_empty());
}

#[test]
fn user_owned_cron_prompt_stays_out_of_bell_and_tracks_waiting_state() {
    let (mut mux, _, child) = two_tabs();
    {
        let tab = mux.get_tab_mut(&child).unwrap();
        tab.hidden = true;
        tab.audience = TabAudience::UserOwnedAutomation;
    }
    let fixture = ServiceFixture::new(mux);
    let cron = Arc::new(Mutex::new(crate::cron::store::CronStore::new_with_path(
        fixture.temp.path().join("crons.json"),
    )));
    fixture._app.manage(cron.clone());
    let run_id = "owner-prompt-run";
    let job_id = {
        let mut store = cron.lock();
        let job = store
            .create(crate::cron::store::CronCreateParams {
                name: "owner-prompt".into(),
                workspace_id: "workspace".into(),
                team_id: None,
                detach_from_team: false,
                schedule: "in 1h".into(),
                task: "task".into(),
                description: None,
                agent: None,
                model: None,
                reasoning_effort: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
                enabled: Some(true),
                max_history: None,
                oneshot: Some(true),
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: None,
            })
            .unwrap();
        store
            .record_run(
                &job.id,
                crate::cron::store::CronRunInfo {
                    run_id: run_id.into(),
                    started_at: 1,
                    finished_at: None,
                    status: crate::cron::store::CronRunStatus::Running,
                    tab_id: Some(child.clone()),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                },
            )
            .unwrap();
        store.register_tab(&child, &job.id, run_id);
        job.id
    };

    let asked = prompt("opaque-owner");
    set_pending_prompt(&fixture, &child, asked.clone());
    assert_eq!(
        fixture
            .service
            .route_pending_prompt(&child, &asked, PromptKind::Provider),
        None
    );
    assert_eq!(
        fixture
            .service
            .route_pending_prompt(&child, &asked, PromptKind::Provider),
        None
    );
    assert!(records_for(&fixture, &child).is_empty());
    assert_eq!(
        cron.lock().get_history(&job_id)[0].status,
        crate::cron::store::CronRunStatus::AwaitingOwnerPrompt
    );

    fixture.service.clear_pending_prompt(&child);
    fixture
        .service
        .resolve_prompt_notifications(&child, &PromptIdentity::new(PromptKind::Provider, &asked));
    let store = cron.lock();
    assert_eq!(
        store.get_history(&job_id)[0].status,
        crate::cron::store::CronRunStatus::Running
    );
    assert_eq!(store.get_history(&job_id)[0].finished_at, None);
}

#[test]
fn restart_and_later_exit_keep_the_bell_empty() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");
    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");
    assert!(open_records_for(&fixture, &child).is_empty());

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
        0,
        "a later exit remains outside the bell"
    );
}

#[test]
fn readiness_keeps_a_prompt_that_is_still_showing() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);
    let asked = prompt("opaque");
    seed_prompt_condition(&fixture, &child);
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
fn tab_close_resolves_exit_and_preexisting_prompt_condition() {
    let (mux, _owner, child) = two_tabs();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .notify_surviving_shell_restart_required(&child, "pty-1");
    seed_prompt_condition(&fixture, &child);
    assert_eq!(open_records_for(&fixture, &child).len(), 1);

    fixture.service.close_tab(&child).unwrap();

    assert!(open_records_for(&fixture, &child).is_empty());
}
