use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

fn prompt(question: &str) -> PendingPrompt {
    PendingPrompt {
        question: question.into(),
        options: vec![PromptOption {
            label: "accept".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

#[test]
fn workspace_close_tears_down_prompt_state_for_every_tab() {
    let (mut mux, first_tab) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&first_tab).unwrap();
    let second_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&first_tab, prompt("first"));
    fixture
        .service
        .prompt_detector
        .lock()
        .hook_detected_explicit(&second_tab, prompt("second"));

    fixture.service.close_workspace(&workspace_id).unwrap();

    let detector = fixture.service.prompt_detector.lock();
    assert!(detector.active_prompt(&first_tab).is_none());
    assert!(detector.active_prompt(&second_tab).is_none());
    drop(detector);
    let mux = fixture.service.mux.read();
    assert!(mux.get_tab(&first_tab).is_none());
    assert!(mux.get_tab(&second_tab).is_none());
}

#[test]
fn workspace_close_keeps_one_workspace_restore_group() {
    let (mut mux, first_tab) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&first_tab).unwrap();
    let second_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);

    fixture.service.close_workspace(&workspace_id).unwrap();

    let view = fixture.service.mux.read().recent_closed_view();
    assert_eq!(view.len(), 2);
    assert!(view.iter().all(|entry| entry.group_kind == "workspace"));
    assert_eq!(view[0].group_id, view[1].group_id);
    assert!(view.iter().any(|entry| entry.tab.tab_id == first_tab));
    assert!(view.iter().any(|entry| entry.tab.tab_id == second_tab));
}

#[test]
fn workspace_close_retires_task_bindings_for_member_tabs() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&owner).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "workspace close cleanup".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "bound worker".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();

    fixture.service.close_workspace(&workspace_id).unwrap();

    let task = fixture
        .service
        .plan_store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(task.assigned_tab_id, None);
    let attempt = task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.executor_tab_id == worker)
        .expect("worker attempt");
    assert!(attempt.closed_at.is_some());
    assert!(attempt.retired_at.is_some());
}

#[test]
fn workspace_close_reservation_blocks_membership_before_lifecycle_cleanup() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&tab_id).unwrap();
    mux.stamp_lifecycle(
        &tab_id,
        crate::session::lifecycle::LifecycleEpoch(7),
        "operation-before-close",
    );
    let fixture = ServiceFixture::new(mux);
    let prepared = fixture
        .service
        .mux
        .write()
        .reserve_workspace_close(&workspace_id)
        .unwrap();

    let create_error = fixture.service.create_tab(&workspace_id).unwrap_err();
    let close_error = fixture.service.close_tab(&tab_id).unwrap_err();

    assert!(create_error.contains("Workspace is closing"));
    assert!(close_error.contains("Workspace is closing"));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.lifecycle_epoch, Some(7));
    assert_eq!(
        tab.lifecycle_operation_id.as_deref(),
        Some("operation-before-close")
    );
    drop(mux);
    fixture
        .service
        .mux
        .write()
        .abort_workspace_close(&prepared);
    assert!(fixture.service.create_tab(&workspace_id).is_ok());
}

#[test]
fn workspace_close_finishes_after_a_pre_teardown_error() {
    let (mut mux, first_tab) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&first_tab).unwrap();
    let second_tab = mux.create_tab(&workspace_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut prepared_tabs = Vec::new();
    let mut finished_tabs = Vec::new();

    let error = fixture
        .service
        .close_workspace_with_teardown(
            &workspace_id,
            |_, tab_id| {
                prepared_tabs.push(tab_id.to_string());
                if tab_id == first_tab.as_str() {
                    Err("injected cleanup failure".to_string())
                } else {
                    Ok(())
                }
            },
            |_, tab_id| {
                finished_tabs.push(tab_id.to_string());
                Ok(())
            },
        )
        .unwrap_err();

    assert!(error.contains("workspace closed with cleanup errors"));
    assert_eq!(prepared_tabs.len(), 2);
    assert_eq!(finished_tabs.len(), 2);
    let mux = fixture.service.mux.read();
    assert!(mux.get_workspace(&workspace_id).is_none());
    assert!(mux.get_tab(&first_tab).is_none());
    assert!(mux.get_tab(&second_tab).is_none());
    let view = mux.recent_closed_view();
    assert_eq!(view.len(), 2);
    assert_eq!(view[0].group_id, view[1].group_id);
}
