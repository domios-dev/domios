use super::*;
use crate::service::SpawnIdentityProvenance;
use std::sync::{Arc, Barrier};

fn scope() -> ReservationScope {
    ReservationScope {
        workspace_id: "ws".into(),
        session_id: Some("sess".into()),
    }
}

#[test]
fn task_bound_planned_name_survives_persona_intent_and_allocation() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        plan_task: Some("task-bound-name".into()),
        title: Some("Eli".into()),
        title_provenance: Some(SpawnIdentityProvenance::Suggested),
        ..Default::default()
    };

    let reservation = fixture
        .service
        .reserve_worker_persona(&mut params)
        .expect("persona reservation succeeds");

    assert!(reservation.is_some());
    assert_eq!(params.title.as_deref(), Some("Eli"));
}

#[test]
fn task_bound_dispatch_without_persona_reserves_a_unique_pool_name() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        task_id: Some("task_bound".into()),
        task_launch_authority: Some(crate::service::TaskLaunchAuthority {
            plan_id: "plan_1".into(),
            task_id: "task_bound".into(),
            attempt_id: "attempt_1".into(),
            lane_base: None,
        }),
        ..Default::default()
    };

    let reservation = fixture
        .service
        .reserve_worker_persona(&mut params)
        .expect("persona reservation succeeds");

    assert!(reservation.is_some());
    let title = params.title.as_deref().expect("task-bound Worker is named");
    assert!(
        WORKER_PERSONA_POOL.contains(&title),
        "dispatch with no persona must name the Worker from the pool, got {title}"
    );
    assert_eq!(
        params.title_provenance,
        Some(SpawnIdentityProvenance::Suggested)
    );
}

#[test]
fn task_bound_explicit_personality_wins_over_pool() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut params = SpawnAgentParams {
        role: Some("Worker".into()),
        task_id: Some("task_bound".into()),
        title: Some("Dumas".into()),
        title_provenance: Some(SpawnIdentityProvenance::UserSupplied),
        task_launch_authority: Some(crate::service::TaskLaunchAuthority {
            plan_id: "plan_1".into(),
            task_id: "task_bound".into(),
            attempt_id: "attempt_1".into(),
            lane_base: None,
        }),
        ..Default::default()
    };

    let reservation = fixture
        .service
        .reserve_worker_persona(&mut params)
        .expect("persona reservation succeeds");

    assert!(reservation.is_some());
    assert_eq!(params.title.as_deref(), Some("Dumas"));
}

#[test]
fn in_flight_persona_is_not_reissued_in_another_workspace() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let ws_a = mux.list_workspaces()[0].id.clone();
    let ws_b = mux.create_workspace("Other".into(), None).id.clone();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    let mut first = SpawnAgentParams {
        role: Some("Worker".into()),
        workspace_id: Some(ws_a),
        ..Default::default()
    };
    let _first_guard = fixture
        .service
        .reserve_worker_persona(&mut first)
        .expect("persona reservation succeeds")
        .expect("a plain Worker is named");
    let first_name = first.title.clone().expect("first Worker is named");

    let mut second = SpawnAgentParams {
        role: Some("Worker".into()),
        workspace_id: Some(ws_b),
        title: Some(first_name.clone()),
        ..Default::default()
    };
    let _second_guard = fixture
        .service
        .reserve_worker_persona(&mut second)
        .expect("persona reservation succeeds")
        .expect("a plain Worker is named");

    assert_ne!(
        second.title.as_deref(),
        Some(first_name.as_str()),
        "an in-flight persona must not be reissued in another workspace"
    );
}

#[test]
fn allocation_avoids_titles_held_in_another_workspace() {
    use crate::multiplexer::state::MultiplexerState;

    let (mut mux, _) = MultiplexerState::new();
    let ws_b = mux.create_workspace("Other".into(), None).id.clone();
    let tab_b = mux
        .create_tab(&ws_b)
        .expect("create tab in other workspace");
    mux.rename_tab(&tab_b, "Ada").expect("rename tab");

    let used = all_live_titles(&mux);
    let allocation = allocate_worker_persona_name(&used, Some("Ada"), None, 0);
    assert_ne!(
        allocation.title, "Ada",
        "a persona held in another workspace must not be reissued"
    );
}

#[test]
fn tab_name_contract_suffix_fallback_only_after_pool_exhaustion() {
    let allocation = allocate_worker_persona_name(WORKER_PERSONA_POOL, None, None, 0);
    assert_eq!(allocation.title, "Worker2");
    assert!(allocation.exhausted_pool);
}

#[test]
fn tab_name_contract_spawn_name_paths_keep_personality_and_role_separate() {
    let task: crate::plan::Task = serde_json::from_value(serde_json::json!({
        "id": "task_name_contract",
        "name": "Stable identity",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "executor_role": "rust-backend-dev"
    }))
    .expect("task fixture deserializes");
    assert_eq!(
        crate::service::planned_task_title(&task).as_deref(),
        Some("Ada")
    );

    let role_only: crate::plan::Task = serde_json::from_value(serde_json::json!({
        "id": "task_role_only",
        "name": "Role stays descriptive",
        "type": "code",
        "status": "pending",
        "executor_role": "rust-backend-dev"
    }))
    .expect("task fixture deserializes");
    assert_eq!(crate::service::planned_task_title(&role_only), None);
}

#[test]
fn concurrent_claims_produce_twenty_unique_names_without_suffix_or_dijkstra() {
    let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
    let barrier = Arc::new(Barrier::new(20));
    let mut threads = Vec::new();
    for idx in 0..20u64 {
        let reservations = reservations.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            let mut lock = reservations.lock();
            let mut used = lock.reserved_titles(&scope());
            let allocation = allocate_worker_persona_name(&used, None, None, idx);
            let token = lock.reserve(scope(), allocation.title.clone());
            used.push(allocation.title.clone());
            (allocation, token.0)
        }));
    }
    let mut seen = HashSet::new();
    let mut reservation_ids = Vec::new();
    for join in threads {
        let (allocation, reservation_id) = join.join().expect("thread join");
        assert!(seen.insert(allocation.title.clone()));
        assert!(WORKER_PERSONA_POOL.contains(&allocation.title.as_str()));
        assert!(!allocation.title.starts_with("Worker "));
        reservation_ids.push(reservation_id);
    }
    assert_eq!(seen.len(), 20);
    let mut lock = reservations.lock();
    for reservation_id in reservation_ids {
        lock.release(reservation_id);
    }
    assert!(lock.entries.is_empty());
}

#[test]
fn pane_self_rename_does_not_release_a_live_worker_persona() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    let (mut mux, orch_tab) = MultiplexerState::new();
    let ws_id = mux.find_workspace_for_tab(&orch_tab).unwrap();
    mux.rename_tab(&orch_tab, "Holst").unwrap();

    let worker = mux.create_tab(&ws_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch_tab))
        .unwrap();

    // Spawn allocates the persona, applies it as the creation title, and
    // commits the claim to the tab id.
    let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
    let token = reservations.lock().reserve(
        ReservationScope {
            workspace_id: ws_id.clone(),
            session_id: None,
        },
        "Vera".into(),
    );
    WorkerPersonaReservationGuard {
        reservations: reservations.clone(),
        reservation_id: Some(token.0),
    }
    .commit_to_tab(&worker);
    mux.rename_tab(&worker, "Vera").unwrap();

    // The pane renames its own tab mid-task — the documented session-start rule.
    mux.rename_tab(&worker, "Identity").unwrap();

    // Exactly what used_worker_persona_names_locked composes.
    let mut reserved = all_live_titles(&mux);
    reserved.extend(reservations.lock().all_reserved_titles());
    assert!(
            reserved.contains("Vera"),
            "a live Worker's assigned persona must stay reserved across its own rename; reserved={reserved:?}"
        );
    assert_ne!(
        allocate_worker_persona_name(&reserved, Some("Vera"), None, 0).title,
        "Vera",
        "persona reissued to a second Worker while the first is still live"
    );
}

#[test]
fn closing_a_renamed_worker_returns_its_persona_to_the_pool() {
    let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
    let token = reservations.lock().reserve(scope(), "Vera".into());
    WorkerPersonaReservationGuard {
        reservations: reservations.clone(),
        reservation_id: Some(token.0),
    }
    .commit_to_tab("tab-1");
    assert_eq!(
        reservations.lock().all_reserved_titles(),
        vec!["Vera".to_string()]
    );

    reservations.lock().release_for_tab("tab-1");
    assert!(
        reservations.lock().all_reserved_titles().is_empty(),
        "a closed tab must not keep holding its persona"
    );
}

#[test]
fn live_titles_include_all_roles_and_workspaces() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    let (mut mux, orch_tab) = MultiplexerState::new();
    let ws_id = mux
        .find_workspace_for_tab(&orch_tab)
        .expect("initial Tab has a workspace");
    mux.rename_tab(&orch_tab, "Orchestrator-Prime")
        .expect("rename orchestrator Tab");

    let plain_tab = mux.create_tab(&ws_id).expect("create plain tab");
    mux.rename_tab(&plain_tab, "Kai").expect("rename Tab");

    let worker_tab = mux.create_tab(&ws_id).expect("create worker tab");
    mux.set_tab_role(&worker_tab, AgentRole::Worker, Some(&orch_tab))
        .expect("attach worker");
    mux.rename_tab(&worker_tab, "Esme").expect("rename worker");

    let other_ws = mux.create_workspace("Other".into(), None).id.clone();
    let other_tab = mux.create_tab(&other_ws).expect("create tab in other ws");
    mux.rename_tab(&other_tab, "Nora").expect("rename tab");

    let titles = all_live_titles(&mux);
    assert!(
            titles.contains("Orchestrator-Prime"),
            "the parent Orchestrator's own tab title must count as used, not just Worker Tabs: {titles:?}"
        );
    assert!(
        titles.contains("Kai"),
        "a plain (roleless) tab's title must count as used: {titles:?}"
    );
    assert!(titles.contains("Esme"));
    assert!(
        titles.contains("Nora"),
        "a title held in a different workspace must count as used: {titles:?}"
    );
}

#[test]
fn live_titles_ignore_session_scoping() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    let (mut mux, orch_tab) = MultiplexerState::new();
    let ws_id = mux.find_workspace_for_tab(&orch_tab).unwrap();

    let other_session_worker = mux.create_tab(&ws_id).unwrap();
    mux.set_tab_role(&other_session_worker, AgentRole::Worker, Some(&orch_tab))
        .unwrap();
    mux.rename_tab(&other_session_worker, "Nora").unwrap();
    if let Some(tab) = mux.get_tab_mut(&other_session_worker) {
        tab.agent_session_id = Some("some-other-session".into());
    }

    // The old scan filtered live tabs to the caller's own session_id,
    // so a Worker title claimed under a different session was invisible.
    let titles = all_live_titles(&mux);
    assert!(
        titles.contains("Nora"),
        "a title held by a Tab from a different session must still be excluded: {titles:?}"
    );
}

#[test]
fn reservations_are_global_across_sessions_and_workspaces() {
    let mut reservations = WorkerPersonaReservations::default();
    reservations.reserve(
        ReservationScope {
            workspace_id: "ws-a".into(),
            session_id: Some("session-a".into()),
        },
        "Ada".into(),
    );

    // A session-scoped lookup cannot see another session's claim...
    assert!(reservations
        .reserved_titles(&ReservationScope {
            workspace_id: "ws-a".into(),
            session_id: Some("session-b".into()),
        })
        .is_empty());
    // ...but the global claim scan must, regardless of session or workspace.
    assert_eq!(reservations.all_reserved_titles(), vec!["Ada".to_string()]);
}

#[test]
fn failed_spawn_rollback_releases_reservation() {
    let reservations = Arc::new(Mutex::new(WorkerPersonaReservations::default()));
    {
        let mut lock = reservations.lock();
        let allocation = allocate_worker_persona_name(std::iter::empty::<&str>(), None, None, 0);
        let token = lock.reserve(scope(), allocation.title.clone());
        drop(lock);
        let guard = WorkerPersonaReservationGuard {
            reservations: reservations.clone(),
            reservation_id: Some(token.0),
        };
        assert_eq!(reservations.lock().entries.len(), 1);
        drop(guard);
    }
    let lock = reservations.lock();
    let allocation = allocate_worker_persona_name(lock.reserved_titles(&scope()), None, None, 0);
    assert_eq!(allocation.title, WORKER_PERSONA_POOL[0]);
    assert!(lock.entries.is_empty());
}
