use super::*;
use crate::store::Owner;
use codeterm_cron::clock::MockClock;
use codeterm_entitlements::{compiled_free, LimitGate};
use codeterm_service_core::service::test_fixture::ServiceFixture;

#[derive(Clone, Copy)]
enum RetainedState {
    Paused,
    Held,
    PlanHeld,
    UncertainHeld,
    Stopped,
}

fn assert_retained(state: RetainedState) {
    for pipeline in [false, true] {
        for team_owned in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (mut mux, creator) = MultiplexerState::new();
            let workspace = mux.find_workspace_for_tab(&creator).unwrap();
            let team = team_owned.then(|| {
                let team = codeterm_types::team::Team::new(&workspace, 1);
                mux.upsert_team(team.clone());
                mux.get_tab_mut(&creator).unwrap().team_id = Some(team.id.clone());
                let teammate = mux.create_tab(&workspace).unwrap();
                mux.get_tab_mut(&teammate).unwrap().team_id = Some(team.id.clone());
                team.id
            });
            let fixture = ServiceFixture::new(mux);
            codeterm_storage::test_support::seed_empty_root(&temp.path().join("storage"));
            let storage =
                codeterm_storage::Storage::open(&temp.path().join("storage"), Default::default())
                    .unwrap();
            let store = Store::with_storage(&storage, MockClock::starting_at(1000));
            let doc = if pipeline {
                json!({"schema_version":2,"kind":"pipeline","name":"retained pipeline","owner":if team_owned { "team" } else { "tab" },
                    "trigger":{"type":"schedule","every":{"lit":"1m"}},
                    "steps":[{"id":"emit","run":{"lit":"printf retained"},"out":{"type":"string"}}],
                    "result":{"to":"owner"}})
            } else {
                json!(codeterm_sentinel::test_support::watch(
                    json!("retained watch"),
                    json!({"uses":"probe","with":{"command":{"lit":"opaque"}},"every":{"lit":"10s"},"paused":true}),
                    json!({"with":{"question":{"lit":"State"}},"buckets":["done"],"matches":{"done":{"lit":"done"}}}),
                    Value::Null,
                    Value::Null,
                ))
            };
            let mut job = store
                .prepare_document(
                    doc,
                    Owner {
                        creator_tab_id: creator,
                        workspace_id: workspace.clone(),
                        team_id: team.clone(),
                        cwd: ".".into(),
                    },
                    false,
                )
                .unwrap();
            if !pipeline {
                assert!(job.scenario_id.is_none());
            }
            job.machine.paused = matches!(state, RetainedState::Paused);
            let (node_state, reason) = match state {
                RetainedState::Paused => (if pipeline { "held" } else { "paused" }, "paused"),
                RetainedState::Held => {
                    job.machine.held = Some("other-transition".into());
                    ("held", "other-transition")
                }
                RetainedState::PlanHeld => {
                    job.admission.reconcile(true, !pipeline);
                    let mut free = compiled_free();
                    free.limits.workspaces.max = Some(0);
                    fixture
                        .service
                        .mux
                        .write()
                        .set_limit_gate(LimitGate::new(free, true));
                    ("held", "plan_limit_locked")
                }
                RetainedState::UncertainHeld => {
                    job.machine.paused = true;
                    job.machine.held = Some("uncertain_pipeline_effect".into());
                    job.last_reason = Some("uncertain_pipeline_effect".into());
                    ("held", "uncertain_pipeline_effect")
                }
                RetainedState::Stopped => {
                    job.machine.stopped = true;
                    job.last_reason = Some("stopped_by_creator".into());
                    ("stopped", "stopped_by_creator")
                }
            };
            let handle =
                crate::scheduler::fixture_handle_with_ui(store, fixture.service.ui.clone());
            let job = storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap();
            crate::manage_fixture_ports(&fixture.service, &handle);
            assert!(graph::reachable_ids(std::slice::from_ref(&job)).contains(&job.id));
            assert!(storage::commit(&handle.store, Store::stage_due)
                .unwrap()
                .is_empty());

            for snapshot in [
                fixture
                    .service
                    .automation_graph_snapshot(&workspace, team.as_deref())
                    .unwrap(),
                fixture
                    .service
                    .automation_graph_workspace_snapshot(&workspace)
                    .unwrap(),
            ] {
                assert_eq!(snapshot.scenarios.len(), 1);
                assert_eq!(snapshot.scenarios[0].root, graph::identity(&job));
                assert_eq!(snapshot.nodes.len(), 1);
                let node = &snapshot.nodes[0];
                assert_eq!(node["availability"], "available");
                assert_eq!(node["state"], node_state);
                assert_eq!(node["detail"]["next_check_at_ms"], Value::Null);
                assert_eq!(snapshot.blueprints.len(), 1);
                let view = &snapshot.blueprints[0];
                assert_eq!(view["id"], job.id);
                if matches!(state, RetainedState::Stopped) {
                    assert_eq!(view["state"], "stopped");
                    assert_eq!(node["stop"]["reason"], reason);
                } else {
                    assert_eq!(view["state"], "held");
                    assert_eq!(node["held_reason"], reason);
                    assert_eq!(view["held_reason"], reason);
                }
                assert!(view["call_graph"].is_object());
            }
            fixture
                .service
                .mux
                .write()
                .remove_tab(&job.creator_tab_id)
                .unwrap();
            let after_owner_close = fixture
                .service
                .automation_graph_workspace_snapshot(&workspace)
                .unwrap();
            if pipeline && team_owned {
                assert_eq!(after_owner_close.blueprints.len(), 1);
            } else {
                assert!(after_owner_close.scenarios.is_empty());
                assert!(after_owner_close.nodes.is_empty());
                assert!(after_owner_close.blueprints.is_empty());
            }
            if let Some(team) = &team {
                fixture.service.mux.write().remove_team(team).unwrap();
                let missing_team = fixture
                    .service
                    .automation_graph_workspace_snapshot(&workspace)
                    .unwrap();
                assert!(missing_team.scenarios.is_empty());
                assert!(missing_team.nodes.is_empty());
                assert!(missing_team.blueprints.is_empty());
                handle.remove_team(team).unwrap();
                assert!(handle.store.lock().get(&job.id).is_none());
                let after_team_cleanup = fixture
                    .service
                    .automation_graph_workspace_snapshot(&workspace)
                    .unwrap();
                assert!(after_team_cleanup.scenarios.is_empty());
                assert!(after_team_cleanup.nodes.is_empty());
                assert!(after_team_cleanup.blueprints.is_empty());
            }
        }
    }
}

#[test]
fn snapshot_admission_keeps_paused_blueprints_without_scheduling() {
    assert_retained(RetainedState::Paused);
}

#[test]
fn snapshot_admission_keeps_held_blueprints_with_reason() {
    assert_retained(RetainedState::Held);
}

#[test]
fn snapshot_admission_keeps_plan_held_blueprints_with_reason() {
    assert_retained(RetainedState::PlanHeld);
}

#[test]
fn snapshot_admission_keeps_uncertain_held_blueprints_with_reason() {
    assert_retained(RetainedState::UncertainHeld);
}

#[test]
fn snapshot_admission_keeps_stopped_blueprints() {
    assert_retained(RetainedState::Stopped);
}
