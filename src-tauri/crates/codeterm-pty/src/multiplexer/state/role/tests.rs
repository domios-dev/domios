use super::*;
use codeterm_types::agent::role_kind::builtins::{GENERAL_AGENT, ORCHESTRATOR, PLANNER, WORKER};

fn fresh() -> (MultiplexerState, String) {
    let (mut mux, _root) = MultiplexerState::new();
    let tab = mux
        .create_tab(&mux.list_workspaces()[0].id.clone())
        .unwrap();
    (mux, tab)
}

fn new_tab(mux: &mut MultiplexerState) -> String {
    mux.create_tab(&mux.list_workspaces()[0].id.clone())
        .unwrap()
}

fn anchor(mux: &mut MultiplexerState, kind: &str) -> String {
    let tab = new_tab(mux);
    mux.set_tab_role_kind(&tab, AgentRole::Manager, Some(kind), None)
        .unwrap_or_else(|error| panic!("{kind} anchor: {error}"));
    tab
}

#[test]
fn detached_worker_role_needs_no_target_local_parent() {
    let (mut mux, tab) = fresh();
    let ownership = codeterm_types::pty::RemoteOwnership {
        controller_title: "Puccini".into(),
        controller_tab_id: Some("af5919cb".into()),
        peer_id: "peer-mac".into(),
        peer_name: "Mac-Air".into(),
        team: None,
    };
    mux.get_tab_mut(&tab).unwrap().ownership = Some(ownership.clone());
    mux.set_detached_worker_role(&tab, None)
        .expect("detached Worker requires no local parent");
    let p = mux.get_tab(&tab).unwrap();
    assert_eq!(p.role, Some(AgentRole::Worker));
    assert_eq!(p.parent_tab_id, None);
    assert!(!p.can_spawn, "leaf Worker is not a manager");
    assert_eq!(p.depth, 2);
    assert_eq!(p.ownership, Some(ownership));
}

#[test]
fn both_anchor_kinds_place_without_a_parent_and_spawn() {
    for kind in [ORCHESTRATOR, GENERAL_AGENT] {
        let (mut mux, _root) = MultiplexerState::new();
        let tab = anchor(&mut mux, kind);
        let p = mux.get_tab(&tab).unwrap();
        assert_eq!(p.role, Some(AgentRole::Manager), "{kind}");
        assert_eq!(p.role_profile.as_deref(), Some(kind));
        assert!(p.can_spawn, "{kind} must default to can_spawn=true");
        assert!(p.anchors_team(), "{kind}");
        assert_eq!(p.depth, 1, "{kind}");
        assert_eq!(p.parent_tab_id, None, "{kind}");
    }
}

#[test]
fn two_anchors_coexist_with_independent_teams() {
    let (mut mux, _root) = MultiplexerState::new();
    let orchestrator = anchor(&mut mux, ORCHESTRATOR);
    let general = anchor(&mut mux, GENERAL_AGENT);

    let w1 = new_tab(&mut mux);
    mux.set_tab_role(&w1, AgentRole::Worker, Some(&orchestrator))
        .expect("worker under the Orchestrator anchor");
    let w2 = new_tab(&mut mux);
    mux.set_tab_role(&w2, AgentRole::Worker, Some(&general))
        .expect("worker under the General Agent anchor");

    assert_eq!(mux.team_count(), 2, "each anchor mints its own Team");
    let orchestrator_team = mux.get_tab(&orchestrator).unwrap().team_id.clone().unwrap();
    let general_team = mux.get_tab(&general).unwrap().team_id.clone().unwrap();
    assert_ne!(orchestrator_team, general_team);
    assert_eq!(
        mux.get_tab(&w1).unwrap().team_id.as_deref(),
        Some(orchestrator_team.as_str())
    );
    assert_eq!(
        mux.get_tab(&w2).unwrap().team_id.as_deref(),
        Some(general_team.as_str())
    );
    assert!(mux
        .get_tab(&orchestrator)
        .unwrap()
        .has_team_orchestrator_authority());
    assert!(mux
        .get_tab(&general)
        .unwrap()
        .has_team_orchestrator_authority());
}

#[test]
fn an_anchor_rejects_a_parent() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = new_tab(&mut mux);
    for kind in [ORCHESTRATOR, GENERAL_AGENT] {
        let child = new_tab(&mut mux);
        let err = mux
            .set_tab_role_kind(&child, AgentRole::Manager, Some(kind), Some(&parent))
            .unwrap_err();
        assert!(err.contains("cannot have a parent"), "{kind}: {err}");
    }
}

#[test]
fn a_profileless_manager_is_rejected_rather_than_defaulting_to_an_anchor() {
    let (mut mux, tab) = fresh();
    let err = mux
        .set_tab_role_kind(&tab, AgentRole::Manager, None, None)
        .unwrap_err();
    assert!(err.contains("named role_profile"), "{err}");
    assert_eq!(mux.get_tab(&tab).unwrap().role, None);
}

#[test]
fn worker_under_free_tab_auto_promotes_to_the_orchestrator_anchor() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = new_tab(&mut mux);
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&parent))
        .expect("worker attaches to a free parent (auto-promote)");
    let p = mux.get_tab(&parent).unwrap();
    assert_eq!(p.role, Some(AgentRole::Manager));
    assert_eq!(p.role_profile.as_deref(), Some(ORCHESTRATOR));
    assert!(p.anchors_team());
    assert!(p.can_spawn);
    let w = mux.get_tab(&worker).unwrap();
    assert_eq!(w.parent_tab_id.as_deref(), Some(parent.as_str()));
    assert_eq!(w.role, Some(AgentRole::Worker));
}

#[test]
fn a_manager_never_auto_promotes_its_parent() {
    let (mut mux, _root) = MultiplexerState::new();
    let free_parent = new_tab(&mut mux);
    let planner = new_tab(&mut mux);
    let err = mux
        .set_tab_role_kind(
            &planner,
            AgentRole::Manager,
            Some(PLANNER),
            Some(&free_parent),
        )
        .unwrap_err();
    assert!(err.contains("requires a spawning parent"), "{err}");
    assert_eq!(
        mux.get_tab(&free_parent).unwrap().role,
        None,
        "a failed Manager placement must not mint an anchor"
    );
    assert_eq!(mux.team_count(), 0);
}

#[test]
fn early_workers_under_uncommitted_anchor_all_link() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = new_tab(&mut mux);
    let w1 = new_tab(&mut mux);
    let w2 = new_tab(&mut mux);
    mux.set_tab_role(&w1, AgentRole::Worker, Some(&parent))
        .expect("first early child");
    mux.set_tab_role(&w2, AgentRole::Worker, Some(&parent))
        .expect("second early child");
    assert_eq!(
        mux.get_tab(&w1).unwrap().parent_tab_id.as_deref(),
        Some(parent.as_str())
    );
    assert_eq!(
        mux.get_tab(&w2).unwrap().parent_tab_id.as_deref(),
        Some(parent.as_str())
    );
    assert_eq!(
        mux.get_tab(&parent).unwrap().role_profile.as_deref(),
        Some(ORCHESTRATOR)
    );
}

#[test]
fn worker_defaults_can_spawn_false() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&root))
        .expect("Worker under the anchor");
    let p = mux.get_tab(&worker).unwrap();
    assert!(!p.can_spawn, "Worker must default to can_spawn=false");
    assert_eq!(p.role_kind_id(), Some(WORKER));
}

#[test]
fn a_worker_subtype_profile_survives_placement_without_gaining_authority() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let worker = new_tab(&mut mux);
    mux.set_tab_role_kind(
        &worker,
        AgentRole::Worker,
        Some("rust-platform-dev"),
        Some(&root),
    )
    .expect("Worker subtype under the anchor");
    let p = mux.get_tab(&worker).unwrap();
    assert_eq!(p.role_profile.as_deref(), Some("rust-platform-dev"));
    assert_eq!(p.role_kind_id(), Some("rust-platform-dev"));
    assert!(!p.anchors_team());
    assert!(!p.has_team_authority());
    assert!(!p.can_spawn);
}

#[test]
fn a_worker_cannot_borrow_an_anchor_kind_through_its_profile() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let worker = new_tab(&mut mux);
    mux.set_tab_role_kind(&worker, AgentRole::Worker, Some(ORCHESTRATOR), Some(&root))
        .expect("placement succeeds but confers no authority");
    let p = mux.get_tab(&worker).unwrap();
    assert_eq!(p.role_kind_id(), Some(WORKER));
    assert!(!p.anchors_team());
    assert!(!p.can_spawn);
    assert_eq!(p.parent_tab_id.as_deref(), Some(root.as_str()));
}

#[test]
fn manager_requires_parent() {
    let (mut mux, tab) = fresh();
    let err = mux
        .set_tab_role_kind(&tab, AgentRole::Manager, Some(PLANNER), None)
        .unwrap_err();
    assert!(err.contains("requires a parent_tab_id"), "{err}");
}

#[test]
fn manager_requires_a_spawning_parent() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&root))
        .unwrap();
    let manager = new_tab(&mut mux);
    let err = mux
        .set_tab_role_kind(&manager, AgentRole::Manager, Some(PLANNER), Some(&worker))
        .unwrap_err();
    assert!(err.contains("requires a spawning parent"), "{err}");
}

#[test]
fn manager_under_an_anchor_is_a_managing_leaf() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let manager = new_tab(&mut mux);
    mux.set_tab_role_kind(&manager, AgentRole::Manager, Some(PLANNER), Some(&root))
        .unwrap();
    let p = mux.get_tab(&manager).unwrap();
    assert_eq!(p.role, Some(AgentRole::Manager));
    assert_eq!(p.role_profile.as_deref(), Some(PLANNER));
    assert_eq!(p.parent_tab_id.as_deref(), Some(root.as_str()));
    assert!(!p.can_spawn, "Manager must default to can_spawn=false");
    assert!(!p.anchors_team());
    assert!(p.has_team_authority(), "a Planner may manage its Team");
}

#[test]
fn manager_can_receive_explicit_spawn_capability() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let manager = new_tab(&mut mux);
    mux.set_tab_role_kind(&manager, AgentRole::Manager, Some(PLANNER), Some(&root))
        .unwrap();
    mux.set_can_spawn(&manager, true).unwrap();
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&manager))
        .unwrap();
    assert_eq!(
        mux.get_tab(&worker).unwrap().parent_tab_id.as_deref(),
        Some(manager.as_str())
    );
}

#[test]
fn manager_cannot_spawn_worker_child() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let manager = new_tab(&mut mux);
    mux.set_tab_role_kind(&manager, AgentRole::Manager, Some(PLANNER), Some(&root))
        .unwrap();
    let child = new_tab(&mut mux);
    let err = mux
        .set_tab_role(&child, AgentRole::Worker, Some(&manager))
        .unwrap_err();
    assert!(err.contains("leaf roles cannot spawn workers"), "{err}");
}

#[test]
fn explicit_set_can_spawn_after_role_does_not_clobber_role() {
    let (mut mux, _root) = MultiplexerState::new();
    let tab = anchor(&mut mux, ORCHESTRATOR);
    mux.set_can_spawn(&tab, false).unwrap();
    let p = mux.get_tab(&tab).unwrap();
    assert_eq!(p.role, Some(AgentRole::Manager));
    assert_eq!(p.role_profile.as_deref(), Some(ORCHESTRATOR));
    assert!(!p.can_spawn);
}

#[test]
fn worker_set_can_spawn_true_is_rejected() {
    let (mut mux, _root) = MultiplexerState::new();
    let root = anchor(&mut mux, ORCHESTRATOR);
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&root))
        .unwrap();
    let err = mux.set_can_spawn(&worker, true).unwrap_err();
    assert!(err.contains("Only managing roles"), "{err}");
}

#[test]
fn no_team_before_first_spawn() {
    let (mut mux, _root) = MultiplexerState::new();
    let tab = new_tab(&mut mux);
    assert_eq!(mux.team_count(), 0, "a roleless Tab mints no team");
    assert_eq!(mux.get_tab(&tab).unwrap().team_id, None);
}

#[test]
fn promoting_an_anchor_without_children_mints_nothing() {
    for kind in [ORCHESTRATOR, GENERAL_AGENT] {
        let (mut mux, _root) = MultiplexerState::new();
        let tab = anchor(&mut mux, kind);
        assert_eq!(
            mux.team_count(),
            0,
            "{kind} with zero children stays teamless"
        );
        assert_eq!(mux.get_tab(&tab).unwrap().team_id, None, "{kind}");
    }
}

#[test]
fn first_worker_spawn_mints_exactly_one_team() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = new_tab(&mut mux);
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&parent))
        .expect("worker under free parent auto-promotes and mints");
    assert_eq!(mux.team_count(), 1);
    let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
    assert!(team_id.starts_with("team_"));
    assert_eq!(
        mux.get_tab(&worker).unwrap().team_id.as_deref(),
        Some(team_id.as_str())
    );
    assert!(mux.get_team(&team_id).is_some());
}

#[test]
fn explicit_anchor_then_first_spawn_mints_once() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = anchor(&mut mux, ORCHESTRATOR);
    assert_eq!(mux.team_count(), 0, "promotion alone does not mint");
    let worker = new_tab(&mut mux);
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&parent))
        .expect("first worker child link");
    assert_eq!(mux.team_count(), 1);
    let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
    assert_eq!(
        mux.get_tab(&worker).unwrap().team_id.as_deref(),
        Some(team_id.as_str())
    );
}

#[test]
fn second_worker_inherits_single_team() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = new_tab(&mut mux);
    let w1 = new_tab(&mut mux);
    let w2 = new_tab(&mut mux);
    mux.set_tab_role(&w1, AgentRole::Worker, Some(&parent))
        .expect("first worker mints");
    mux.set_tab_role(&w2, AgentRole::Worker, Some(&parent))
        .expect("second worker inherits");
    assert_eq!(mux.team_count(), 1);
    let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
    assert_eq!(
        mux.get_tab(&w1).unwrap().team_id.as_deref(),
        Some(team_id.as_str())
    );
    assert_eq!(
        mux.get_tab(&w2).unwrap().team_id.as_deref(),
        Some(team_id.as_str())
    );
}

#[test]
fn manager_child_link_mints_team() {
    let (mut mux, _root) = MultiplexerState::new();
    let parent = anchor(&mut mux, ORCHESTRATOR);
    let planner = new_tab(&mut mux);
    mux.set_tab_role_kind(&planner, AgentRole::Manager, Some(PLANNER), Some(&parent))
        .expect("planner child link");
    assert_eq!(mux.team_count(), 1);
    let team_id = mux.get_tab(&parent).unwrap().team_id.clone().unwrap();
    assert_eq!(
        mux.get_tab(&planner).unwrap().team_id.as_deref(),
        Some(team_id.as_str())
    );
}

#[test]
fn concurrent_first_spawns_single_team() {
    use parking_lot::Mutex;
    use std::sync::Arc;

    for _ in 0..100 {
        let (mut m, _root) = MultiplexerState::new();
        let parent = m.create_tab(&m.list_workspaces()[0].id.clone()).unwrap();
        let c1 = m.create_tab(&m.list_workspaces()[0].id.clone()).unwrap();
        let c2 = m.create_tab(&m.list_workspaces()[0].id.clone()).unwrap();
        let state = Arc::new(Mutex::new(m));

        let (s1, s2) = (Arc::clone(&state), Arc::clone(&state));
        let (p1, p2) = (parent.clone(), parent.clone());
        let (child1, child2) = (c1.clone(), c2.clone());
        let t1 = std::thread::spawn(move || {
            s1.lock()
                .set_tab_role(&child1, AgentRole::Worker, Some(&p1))
                .unwrap();
        });
        let t2 = std::thread::spawn(move || {
            s2.lock()
                .set_tab_role(&child2, AgentRole::Worker, Some(&p2))
                .unwrap();
        });
        t1.join().unwrap();
        t2.join().unwrap();

        let guard = state.lock();
        assert_eq!(guard.team_count(), 1);
        let team_id = guard.get_tab(&parent).unwrap().team_id.clone().unwrap();
        assert_eq!(
            guard.get_tab(&c1).unwrap().team_id.as_deref(),
            Some(team_id.as_str())
        );
        assert_eq!(
            guard.get_tab(&c2).unwrap().team_id.as_deref(),
            Some(team_id.as_str())
        );
    }
}
