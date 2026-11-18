use super::*;
use codeterm_service_core::mesh::types::{AccessLevel, PairedPeer};
use codeterm_service_core::service::plan::plan_skills::{self, NewPlanSkill};
use codeterm_service_core::service::test_fixture::ServiceFixture;

#[test]
fn refused_remote_skill_context_leaves_task_unassigned_without_an_attempt() {
    let (_root, project) = git_project();
    git(
        &project,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/opaque/repository.git",
        ],
    );
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let _storage = fixture.install_seeded_storage_desk();
    let svc = &fixture.service;
    let mut plan = svc
        .plan_store
        .create_plan_for_team(
            "skill preflight".into(),
            project.display().to_string(),
            OrchestratorRef {
                tab_id: Some(owner.clone()),
                session_id: Some("owner-session".into()),
                title: None,
            },
            team,
            vec![],
            1,
        )
        .unwrap();
    plan.mode = PlanMode::Approved;
    plan.planning_state = codeterm_service_core::plan::PlanningState::Approved;
    svc.plan_store.save(&mut plan, 2).unwrap();
    let task_id = svc
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque task".into(),
                personality: Some("Ada".into()),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    plan_skills::create_plan_skill(
        &svc.plan_store,
        &plan.id,
        NewPlanSkill {
            slug: "opaque".into(),
            title: None,
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let skill = plan_skills::list_plan_skills(&svc.plan_store, &plan.id)
        .unwrap()
        .remove(0);
    std::fs::write(
        Path::new(&skill.skill_dir).join("SKILL.md"),
        "x".repeat(256 * 1024),
    )
    .unwrap();
    svc.ext::<codeterm_service_core::MeshStateHandle>()
        .lock()
        .paired_peers
        .insert(
            "peer-id".into(),
            PairedPeer {
                instance_id: "peer-id".into(),
                instance_name: "Peer".into(),
                online: false,
                addresses: vec![],
                api_port: 7686,
                public_key: "key".into(),
                paired_at: "now".into(),
                last_seen: None,
                is_primary: false,
                access_level: AccessLevel::Full,
                domios_version: None,
                domios_build: None,
                web_ui: None,
                relay_protocol_version: 1,
                consecutive_misses: 0,
                last_active_ms: None,
            },
        );
    let before = svc.plan_store.inspect_task_for_test(&task_id).unwrap().task;
    let error = super::super::remote::dispatch(
        &Some(json!({
            "plan_id": plan.id, "task_id": task_id, "mesh_peer": "peer-id",
            "execution_intent": "mutating", "caller_tab_id": owner,
            "session_id": "owner-session", "session_binding_nonce": "owner-nonce"
        })),
        svc,
        4,
    )
    .unwrap_err();
    assert!(
        error.message.contains("skill context exceeds 256 KiB"),
        "{error:?}"
    );
    let after = svc.plan_store.inspect_task_for_test(&task_id).unwrap().task;
    assert!(after.assigned_tab_id.is_none());
    assert!(after.execution_attempts.is_empty());
    assert_eq!(after.lane, before.lane);
    assert_eq!(after.status, before.status);
    assert_eq!(after.updated_at, before.updated_at);
}
