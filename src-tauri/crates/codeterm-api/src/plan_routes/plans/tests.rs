use super::*;
use codeterm_service_core::plan::{OrchestratorRef, Plan, PlanStatus};

fn mk_index_entry(plan_id: &str, file_path: PathBuf, status: PlanStatus) -> IndexEntry {
    IndexEntry {
        plan_id: plan_id.to_string(),
        file_path,
        canonical_project_path: "/repo".into(),
        owner: OrchestratorRef::default(),
        team_id: None,
        title: "Indexed plan".into(),
        status,
        updated_at: 1,
        assigned_tab_ids: vec![],
        task_count: 0,
        open_task_count: 0,
        section_count: 0,
        element_count: 0,
        loop_count: 0,
        has_goal: false,
        orphaned: None,
    }
}

fn mk_plan(plan_id: &str) -> Plan {
    let mut plan = Plan::new(
        "Resolved plan".into(),
        "/repo".into(),
        OrchestratorRef::default(),
    );
    plan.id = plan_id.into();
    plan
}

#[test]
fn derive_project_root_accepts_canonical_sidecar_path() {
    let root = derive_project_root_from_index_path(
        FsPath::new("/repo/.domios/plans/active/pl_test/plan.yaml"),
        "pl_test",
    );
    assert_eq!(root, Some(PathBuf::from("/repo")));
}

#[test]
fn derive_project_root_rejects_non_sidecar_path() {
    let root = derive_project_root_from_index_path(
        FsPath::new("/repo/.domios/plans/active/pl_other/plan.yaml"),
        "pl_test",
    );
    assert!(root.is_none());
}

#[test]
fn load_plan_for_ref_hydrates_cross_project_index_hit() {
    let entry = mk_index_entry(
        "pl_test",
        PathBuf::from("/project-b/.domios/plans/active/pl_test/plan.yaml"),
        PlanStatus::Active,
    );
    let loaded = std::cell::RefCell::new(None::<Plan>);
    let (plan, state) = load_plan_for_ref(
        "pl_test",
        Some(&entry),
        || loaded.borrow().clone(),
        |_| {
            *loaded.borrow_mut() = Some(mk_plan("pl_test"));
            Ok(())
        },
    );
    assert!(state.is_none());
    assert_eq!(plan.as_ref().map(|p| p.id.as_str()), Some("pl_test"));
}

#[test]
fn load_plan_for_ref_reports_permission_for_confined_bad_path() {
    let entry = mk_index_entry(
        "pl_test",
        PathBuf::from("/project-b/outside/pl_test.txt"),
        PlanStatus::Active,
    );
    let (plan, state) = load_plan_for_ref("pl_test", Some(&entry), || None, |_| Ok(()));
    assert!(plan.is_none());
    assert_eq!(state, Some("permission"));
}

#[test]
fn load_plan_for_ref_reports_unreachable_from_loader() {
    let entry = mk_index_entry(
        "pl_test",
        PathBuf::from("/project-b/.domios/plans/active/pl_test/plan.yaml"),
        PlanStatus::Active,
    );
    let (plan, state) = load_plan_for_ref("pl_test", Some(&entry), || None, |_| Err("unreachable"));
    assert!(plan.is_none());
    assert_eq!(state, Some("unreachable"));
}

#[test]
fn load_plan_for_ref_reports_missing_without_index() {
    let (plan, state) = load_plan_for_ref("pl_test", None, || None, |_| Ok(()));
    assert!(plan.is_none());
    assert_eq!(state, Some("missing"));
}

#[test]
fn archived_plan_reference_resolves_without_missing_state() {
    let mut archived = mk_plan("pl_archived");
    archived.status = PlanStatus::Archived;
    let archived_entry = mk_index_entry(
        "pl_archived",
        PathBuf::from("/repo/.domios/plans/archive/pl_archived/plan.yaml"),
        PlanStatus::Archived,
    );
    let (archived_plan, archived_state) = load_plan_for_ref(
        "pl_archived",
        Some(&archived_entry),
        || Some(archived.clone()),
        |_| Ok(()),
    );
    assert_eq!(archived_state, None);
    assert_eq!(archived_plan.expect("plan").status, PlanStatus::Archived);
}

#[test]
fn transfer_caller_requires_agent_header() {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", "source-manager".parse().unwrap());
    assert_eq!(
        transfer_caller(&crate::auth::ActorHeaders::for_owner_test(headers.clone())).unwrap(),
        "source-manager"
    );
    let missing =
        transfer_caller(&crate::auth::ActorHeaders::for_owner_test(HeaderMap::new())).unwrap_err();
    assert_eq!(missing.0, StatusCode::FORBIDDEN);
    assert_eq!(
        team_plan_err(TeamPlanError::Team(
            "Caller tab worker is not authorized to manage Team team_source".into(),
        ))
        .0,
        StatusCode::FORBIDDEN
    );
}
