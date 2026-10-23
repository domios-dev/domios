use super::*;
use crate::store::{Owner, Store};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;

#[test]
fn workspace_graph_projection_releases_mux_before_building() {
    let (mux, tab_id) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab_id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    with_graph_mux(svc, &workspace, None, &[], std::iter::empty(), |mux| {
        assert!(
            svc.mux.try_write().is_some(),
            "graph construction holds the mux read lock"
        );
        let snapshot = graph::project(
            &workspace,
            None,
            &[],
            mux,
            &BTreeMap::new(),
            Value::Null,
            1000,
            &BTreeSet::new(),
        )?;
        assert_eq!(snapshot.workspace_id, workspace);
        Ok(())
    })
    .unwrap();
}

#[test]
fn graph_mux_snapshot_preserves_referenced_tabs_and_team_scope() {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    let unrelated = mux.create_tab(&workspace).unwrap();
    let mut team = codeterm_types::team::Team::new(&workspace, 1);
    team.name = Some("Scoped team".into());
    mux.upsert_team(team.clone());
    for id in [&creator, &worker] {
        mux.get_tab_mut(id).unwrap().team_id = Some(team.id.clone());
    }
    mux.get_tab_mut(&worker).unwrap().title = "Captured worker".into();
    mux.get_tab_mut(&worker).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let store = Store::new(
        fixture.temp.path().join("graph.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let job = store.prepare_document(
        json!(codeterm_sentinel::test_support::watch(
            json!("Snapshot scenario"),
            json!({"uses":"probe","with":{"command":{"lit":"fixture"}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}}),
            json!({"with":{"question":{"lit":"State"}},"buckets":["done"],"matches":{"done":{"lit":"done"}}}),
            json!({"done":[{"id":"send","uses":"send","with":{"target":{"lit":worker}}}]}),
            Value::Null,
        )),
        Owner {creator_tab_id: creator.clone(), team_id: Some(team.id.clone()), workspace_id: workspace.clone(), cwd: ".".into()},
        false,
    ).unwrap();
    let jobs = vec![job];
    let crons = BTreeMap::new();
    let project = |view: &dyn graph::MuxView| {
        graph::project(
            &workspace,
            Some(&team.id),
            &jobs,
            view,
            &crons,
            Value::Null,
            1000,
            &BTreeSet::new(),
        )
    };
    let expected = project(&*svc.mux.read()).unwrap();
    with_graph_mux(
        svc,
        &workspace,
        Some(&team.id),
        &jobs,
        crons.values(),
        |view| {
            assert!(view.get_tab(&unrelated).is_none());
            let mut current = svc
                .mux
                .try_write()
                .expect("graph snapshot retained the mux guard");
            current.get_tab_mut(&worker).unwrap().team_id = None;
            current.get_tab_mut(&worker).unwrap().title = "Changed worker".into();
            drop(current);
            let mut snapshot = project(view)?;
            assert_eq!(
                serde_json::to_value(&snapshot).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            let resolved = std::cell::RefCell::new(Vec::new());
            graph::project_worker_states(&mut snapshot, view, |tab| {
                assert!(svc.mux.try_write().is_some());
                resolved.borrow_mut().push(tab.id.clone());
                Some(codeterm_service_core::agent::status::TabStatus::Working)
            });
            assert_eq!(*resolved.borrow(), vec![worker.clone()]);
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node["identity"]["id"] == worker)
                .unwrap();
            assert_eq!(node["state_source"], "tab_status");
            assert_eq!(node["name"], "Captured worker");
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn graph_mux_snapshot_includes_cron_workers_and_excludes_foreign_tabs() {
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    let foreign_workspace = mux.create_workspace("Foreign".into(), None).id;
    let foreign = mux.create_tab(&foreign_workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let crons: BTreeMap<String, ConnectedCron> = BTreeMap::from([(
        "cron".into(),
        ConnectedCron {
            job: None,
            identity: Identity {
                kind: "cron".into(),
                id: "cron".into(),
                workspace_id: workspace.clone(),
                team_id: None,
                owner_tab_id: Some(creator),
            },
            runs: [worker.clone(), foreign.clone()]
                .into_iter()
                .map(|tab| codeterm_service_core::cron::store::CronRunInfo {
                    run_id: tab.clone(),
                    started_at: 1000,
                    finished_at: None,
                    status: codeterm_service_core::cron::store::CronRunStatus::Running,
                    tab_id: Some(tab),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                })
                .collect(),
        },
    )]);
    with_graph_mux(
        &fixture.service,
        &workspace,
        None,
        &[],
        crons.values(),
        |view| {
            assert_eq!(view.get_tab(&worker).unwrap().id, worker);
            assert!(view.get_tab(&foreign).is_none());
            assert!(view.find_workspace_for_tab(&foreign).is_none());
            Ok(())
        },
    )
    .unwrap();
}
