use super::*;
use crate::multiplexer::types::TabAudience;
use crate::service::test_fixture::ServiceFixture;
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};
use codeterm_types::pty::{
    AgentRole, BootStage, PendingPrompt, PromptIdentity, PromptKind, PromptOption,
};

#[test]
fn template_manager_prompt_notifies_before_owner_wait_stage() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let manager = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&manager).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&manager)
        .unwrap()
        .team_id;
    let params = SpawnAgentParams {
        agent: Some("opaque-harness".into()),
        role: Some("Orchestrator".into()),
        can_spawn: Some(true),
        workspace_id: Some(workspace),
        user_created: true,
        existing_tab_id: Some(manager.clone()),
        skip_orchestrator_onboarding: Some(true),
        ..Default::default()
    };
    initialize_ordinary_pty_agent_tab(mux.get_tab_mut(&manager).unwrap(), &params);
    mux.set_orchestrator_role(&manager).unwrap();
    let tab = mux.get_tab(&manager).unwrap();
    assert_eq!(tab.audience, TabAudience::User);
    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(tab.team_id.as_deref(), Some(team.as_str()));
    assert!(tab.agent_booting);

    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .emit_boot_stage(&manager, BootStage::LaunchWritten, None);
    let prompt = PendingPrompt {
        request_id: None,
        question: "Opaque external-file question".into(),
        options: vec![
            PromptOption {
                label: "Opaque decline".into(),
                value: "decline-value".into(),
            },
            PromptOption {
                label: "Opaque allow".into(),
                value: "allow-value".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&manager).unwrap();
        tab.pending_prompt = Some(prompt.clone());
        tab.pending_prompt_kind = Some(PromptKind::Provider);
    }
    fixture
        .service
        .route_pending_prompt(&manager, &prompt, PromptKind::Provider);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&manager)
            .unwrap()
            .boot_stage,
        Some(BootStage::LaunchWritten)
    );
    let first_notice = {
        let store = fixture.service.notif.lock();
        let records = store.list(false);
        let notices = records
            .iter()
            .filter(|notice| notice.tab_id.as_deref() == Some(manager.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(notices.len(), 1, "the first route must store the notification before the readiness observer changes the stage");
        assert_eq!(notices[0].level, Some(NotifyLevel::ActionNeeded));
        notices[0].id.clone()
    };
    fixture
        .service
        .emit_boot_stage(&manager, BootStage::AwaitingOwnerPrompt, None);
    fixture
        .service
        .route_pending_prompt(&manager, &prompt, PromptKind::Provider);
    let identity = PromptIdentity::new(PromptKind::Provider, &prompt);
    let notification_id = {
        let store = fixture.service.notif.lock();
        let records = store.list(false);
        let notices = records
            .iter()
            .filter(|notice| notice.tab_id.as_deref() == Some(manager.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].id, first_notice);
        assert_eq!(notices[0].level, Some(NotifyLevel::ActionNeeded));
        assert_eq!(
            notices[0].actions,
            Some(
                prompt
                    .options
                    .iter()
                    .map(|option| NotificationAction::RespondPromptOption {
                        label: option.label.clone(),
                        value: option.value.clone(),
                        identity: identity.clone(),
                    })
                    .collect()
            )
        );
        notices[0].id.clone()
    };
    fixture
        .service
        .answer_prompt_notifications(&manager, &identity, 1, "test-owner");
    {
        let store = fixture.service.notif.lock();
        let records = store.list(false);
        let answered = records
            .iter()
            .find(|notice| notice.id == notification_id)
            .unwrap();
        assert!(answered.resolved);
    }
    fixture.service.clear_pending_prompt(&manager);
    fixture
        .service
        .resolve_prompt_notifications(&manager, &identity);
}
