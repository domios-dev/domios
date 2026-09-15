use super::{
    ai_helper_delivery, ai_helper_params, ai_helper_placement, AiHelperDelivery, AiHelperPlacement,
    AiHelperRequest,
};
use crate::multiplexer::state::MultiplexerState;
use crate::service::general_agent::{ensure_general_workspace, general_dir, general_workspace_id};
use crate::service::launch_delivery::select_visible_spawn_task;
use crate::service::spawn::{SpawnTabSpec, TabCommand, TabVisibility};

fn activate_project_workspace(mux: &mut MultiplexerState) -> String {
    let project = mux.create_workspace(
        "ai-helper-spawn-project".into(),
        Some("ai-helper-spawn-project-cwd".into()),
    );
    let _ = mux.set_active_workspace(&project.id);
    assert_eq!(
        mux.get_active_workspace()
            .map(|workspace| workspace.id.clone()),
        Some(project.id.clone())
    );
    project.id
}

#[test]
fn ai_helper_spawn_places_the_tab_in_general_not_the_active_workspace() {
    let mut mux = MultiplexerState::empty();
    ensure_general_workspace(&mut mux, false);
    let active = activate_project_workspace(&mut mux);
    let general = general_workspace_id(&mux).expect("General workspace");

    let placement = ai_helper_placement(&mux).expect("helper placement");

    assert_eq!(placement.workspace_id, general);
    assert_ne!(placement.workspace_id, active);
    assert_eq!(placement.cwd, general_dir().to_string_lossy());
}

#[test]
fn ai_helper_spawn_without_general_workspace_is_an_explicit_error() {
    let mut mux = MultiplexerState::empty();
    activate_project_workspace(&mut mux);

    assert!(ai_helper_placement(&mux).is_err());
}

#[test]
fn ai_helper_spawn_spec_is_a_roleless_visible_launch_in_general() {
    let request =
        AiHelperRequest::new("ai-helper-title-7c1".into(), "ai-helper-task-3e9".into()).unwrap();
    let placement = AiHelperPlacement {
        workspace_id: "ws-general-fixture".into(),
        cwd: "general-cwd-fixture".into(),
    };

    let spec =
        SpawnTabSpec::from_agent_params(ai_helper_params(&request, &placement, "provider-fixture"));

    assert_eq!(spec.workspace_id.as_deref(), Some("ws-general-fixture"));
    assert_eq!(spec.cwd.as_deref(), Some("general-cwd-fixture"));
    assert_eq!(spec.title.as_deref(), Some("ai-helper-title-7c1"));
    assert_eq!(spec.parent_tab_id, None);
    assert_eq!(spec.visibility, TabVisibility::Visible);
    let TabCommand::Agent(agent) = spec.command else {
        panic!("helper spawn must launch an agent command");
    };
    assert_eq!(agent.provider, "provider-fixture");
    assert_eq!(agent.task.as_deref(), Some("ai-helper-task-3e9"));
    assert_eq!(agent.role, None);
    assert_eq!(agent.spawned_by_tab_id, None);
    assert_eq!(
        agent.provisioned_cwd.as_deref(),
        Some("general-cwd-fixture")
    );
    assert_eq!(agent.activate, Some(true));
}

#[test]
fn ai_helper_spawn_always_starts_a_first_turn() {
    for (system_prompt_capable, native_starter_capable) in
        [(true, true), (true, false), (false, true), (false, false)]
    {
        let selection = select_visible_spawn_task(
            None,
            Some("ai-helper-task-3e9"),
            system_prompt_capable,
            native_starter_capable,
        );
        assert!(
            selection.provider_starter.is_some() || selection.confirmed_first_turn.is_some(),
            "no first turn for system_prompt={system_prompt_capable} native_starter={native_starter_capable}"
        );
    }
}

#[test]
fn ai_helper_spawn_never_reports_confirmed_for_an_inline_first_turn() {
    assert_eq!(
        ai_helper_delivery(true, true, "ai-helper-task-3e9"),
        AiHelperDelivery::Launched
    );
    assert_eq!(
        ai_helper_delivery(true, false, "ai-helper-task-3e9"),
        AiHelperDelivery::Confirmed
    );
    assert_eq!(
        ai_helper_delivery(false, false, "ai-helper-task-3e9"),
        AiHelperDelivery::Confirmed
    );
    assert_ne!(
        AiHelperDelivery::Launched.as_str(),
        AiHelperDelivery::Confirmed.as_str()
    );
}

#[test]
fn ai_helper_spawn_request_rejects_blank_title_or_task() {
    assert!(AiHelperRequest::new("  ".into(), "task".into()).is_err());
    assert!(AiHelperRequest::new("title".into(), " \n".into()).is_err());
    assert!(AiHelperRequest::new("title".into(), "task".into()).is_ok());
}
