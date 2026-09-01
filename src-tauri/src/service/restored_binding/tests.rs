use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, Plan, PlannerRef};
use crate::service::restored_binding::RestoreBindingError;
use crate::service::test_fixture::ServiceFixture;

/// Owner tab bound to `session`, plus a Planner tab on the same plan.
fn fixture_with_plan() -> (ServiceFixture, String, String, String) {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("restore".into(), None)
        .id;
    let owner = fixture.service.mux.write().create_tab(&workspace).unwrap();
    let planner = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.title = "Dumas".into();
        tab.agent_session_id = Some("owner-session".into());
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.title = "Euler".into();
        tab.agent_session_id = Some("planner-session".into());
    }
    let mut plan = Plan::new(
        "restore".into(),
        "/repo".into(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            // Stale identity from the epoch that died.
            session_id: Some("dead-session".into()),
            title: Some("Old Title".into()),
        },
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: planner.clone(),
        session_id: Some("dead-planner-session".into()),
        title: Some("Old Planner".into()),
    });
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let plan_id = plan.id.clone();
    (fixture, owner, planner, plan_id)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn owner_binding_is_refreshed_to_the_tabs_own_session() {
    let (fixture, owner, _, plan_id) = fixture_with_plan();

    let restored = fixture
        .service
        .restore_caller_binding(&owner, Some("owner-session"))
        .expect("binding restores against the tab's own session");

    assert_eq!(restored.owned_plan_ids, vec![plan_id.clone()]);
    assert!(restored.planner_plan_ids.is_empty());
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.owner.session_id.as_deref(), Some("owner-session"));
    assert_eq!(plan.owner.title.as_deref(), Some("Dumas"));
    assert_eq!(plan.owner.tab_id.as_deref(), Some(owner.as_str()));
}

/// A restart is not a handover, so refreshing the owner's live session must not
/// append a transfer record.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn refreshing_the_owner_records_no_transfer_history() {
    let (fixture, owner, _, plan_id) = fixture_with_plan();
    let before = fixture.service.plan_store.get(&plan_id).unwrap();

    fixture
        .service
        .restore_caller_binding(&owner, Some("owner-session"))
        .expect("restore");

    let after = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(after.owner_history.len(), before.owner_history.len());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn planner_binding_is_refreshed_to_the_tabs_own_session() {
    let (fixture, _, planner, plan_id) = fixture_with_plan();

    let restored = fixture
        .service
        .restore_caller_binding(&planner, Some("planner-session"))
        .expect("restore");

    assert_eq!(restored.planner_plan_ids, vec![plan_id.clone()]);
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    match plan.planner {
        Some(PlannerRef::Tab {
            tab_id,
            session_id,
            title,
        }) => {
            assert_eq!(tab_id, planner);
            assert_eq!(session_id.as_deref(), Some("planner-session"));
            assert_eq!(title.as_deref(), Some("Euler"));
        }
        other => panic!("planner ref lost: {other:?}"),
    }
}

/// The core of the incident: a tab that came back on a different session must
/// never have plan/task callers re-pointed at it.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn a_foreign_session_fails_the_restore_and_leaves_the_plan_untouched() {
    let (fixture, owner, _, plan_id) = fixture_with_plan();

    let error = fixture
        .service
        .restore_caller_binding(&owner, Some("the-authoritative-one"))
        .expect_err("a foreign session must not be adopted");

    assert!(
        matches!(
            error,
            RestoreBindingError::ForeignSession {
                ref expected,
                ref live,
                ..
            } if expected == "the-authoritative-one" && live == "owner-session"
        ),
        "{error:?}"
    );
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(
        plan.owner.session_id.as_deref(),
        Some("dead-session"),
        "the stale binding survives rather than being rebound to a foreign session"
    );
}

/// A fresh (non-resumed) launch legitimately returns on a new session, so no
/// expectation is imposed and the binding still lands.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn a_fresh_launch_binds_without_an_expected_session() {
    let (fixture, owner, _, plan_id) = fixture_with_plan();

    let restored = fixture
        .service
        .restore_caller_binding(&owner, None)
        .expect("a fresh launch still rebinds its own callers");

    assert_eq!(restored.agent_session_id.as_deref(), Some("owner-session"));
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.owner.session_id.as_deref(), Some("owner-session"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn a_roleless_tab_restores_an_empty_binding_without_touching_any_plan() {
    let (fixture, _, _, plan_id) = fixture_with_plan();
    let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
    let stranger = fixture.service.mux.write().create_tab(&workspace).unwrap();

    let restored = fixture
        .service
        .restore_caller_binding(&stranger, None)
        .expect("restore");

    assert!(restored.is_empty());
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.owner.session_id.as_deref(), Some("dead-session"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn a_missing_tab_fails_rather_than_resolving_a_global_session() {
    let (fixture, _, _, _) = fixture_with_plan();

    let error = fixture
        .service
        .restore_caller_binding("no-such-tab", None)
        .expect_err("an absent tab has no authoritative session to restore");

    assert!(
        matches!(error, RestoreBindingError::TabNotFound { ref tab_id } if tab_id == "no-such-tab"),
        "{error:?}"
    );
}

/// A relaunched tab with no reported session yet is "not yet known", never
/// "wrong" — collapsing the two is the incident this gate closes.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn unreported_session_is_not_a_foreign_session() {
    let (fixture, owner, _, plan_id) = fixture_with_plan();
    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&owner).unwrap().agent_session_id = None;
    }
    let before = fixture.service.plan_store.get(&plan_id).unwrap();

    let restored = fixture
        .service
        .restore_caller_binding(&owner, Some("owner-session"))
        .expect("an unreported session must not be refused as a foreign one");

    assert_eq!(restored.agent_session_id, None);
    assert!(restored.owned_plan_ids.is_empty());
    assert!(restored.planner_plan_ids.is_empty());

    let after = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(
        after.owner.session_id, before.owner.session_id,
        "an unconfirmed session must not overwrite the plan's existing owner binding"
    );
    assert_eq!(
        after.owner.tab_id, before.owner.tab_id,
        "plan owner binding for the tab is untouched while its session is unconfirmed"
    );
    assert_eq!(
        after.owner_history.len(),
        before.owner_history.len(),
        "no transfer history is recorded for an unconfirmed restore"
    );
}
