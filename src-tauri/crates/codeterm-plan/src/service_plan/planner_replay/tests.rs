use super::*;
use crate::plan::PlannerRequest;

fn plan_with(planner: Option<PlannerRef>, states: &[PlannerRequestState]) -> Plan {
    plan_in("/project", planner, states)
}

fn plan_in(project: &str, planner: Option<PlannerRef>, states: &[PlannerRequestState]) -> Plan {
    let mut plan = Plan::new("P".into(), project.into(), OrchestratorRef::default());
    plan.planner = planner;
    plan.planner_requests = states
        .iter()
        .enumerate()
        .map(|(index, state)| request(&format!("req-{index}"), state.clone()))
        .collect();
    plan
}

fn request(request_id: &str, state: PlannerRequestState) -> PlannerRequest {
    PlannerRequest {
        request_id: request_id.into(),
        payload: "opaque".into(),
        payload_blob: None,
        caller_tab_id: None,
        caller_session_id: None,
        state,
        created_at: 1,
        accepted_at: None,
        lease_expires_at: None,
        delivery_attempts: 0,
        completed_at: None,
        failed_at: None,
        delivered_to_tab_id: None,
        delivered_to_session_id: None,
        delivered_with_binding_nonce: None,
    }
}

fn tab(tab_id: &str, session_id: Option<&str>) -> Option<PlannerRef> {
    Some(PlannerRef::Tab {
        tab_id: tab_id.into(),
        session_id: session_id.map(Into::into),
        title: None,
    })
}

#[test]
fn candidate_requires_bound_tab_session_and_open_request() {
    let open = [PlannerRequestState::Completed, PlannerRequestState::Queued];
    let selected = plan_with(tab("planner", Some("s1")), &open);
    let candidate = planner_replay_candidate(&selected, "planner").expect("bound open plan");
    assert_eq!(candidate.plan_id, selected.id);
    assert_eq!(candidate.session_id, "s1");
    assert_eq!(candidate.planner, selected.planner);

    assert!(planner_replay_candidate(&selected, "other").is_none());
    assert!(planner_replay_candidate(&plan_with(tab("planner", None), &open), "planner").is_none());
    assert!(planner_replay_candidate(&plan_with(None, &open), "planner").is_none());
    let waived = Some(PlannerRef::Waived {
        note: "solo".into(),
    });
    assert!(planner_replay_candidate(&plan_with(waived, &open), "planner").is_none());
    let done = [PlannerRequestState::Completed];
    assert!(
        planner_replay_candidate(&plan_with(tab("planner", Some("s1")), &done), "planner")
            .is_none()
    );
    assert!(
        planner_replay_candidate(&plan_with(tab("planner", Some("s1")), &[]), "planner").is_none()
    );
}

#[test]
fn store_query_projects_only_the_bound_plan_among_unrelated_plans() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = dir.path().display().to_string();
    let queued = [PlannerRequestState::Queued];
    let mut bound = plan_in(&project, tab("planner", Some("s1")), &queued);
    store.save(&mut bound, 1).unwrap();
    for mut unrelated in [
        plan_in(&project, tab("other", Some("s2")), &queued),
        plan_in(
            &project,
            tab("planner", Some("s1")),
            &[PlannerRequestState::Completed],
        ),
        plan_in(&project, None, &queued),
    ] {
        store.save(&mut unrelated, 1).unwrap();
    }

    let candidates = store.planner_replay_candidates("planner");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].plan_id, bound.id);
    assert!(store.planner_replay_candidates("nobody").is_empty());
}
