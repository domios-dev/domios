use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;

use super::handlers::{Handler, Status};
use super::machine::{Route, Verdict};
use super::provenance::Classification;
use super::store::{Owner, Sentinel, Store};

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

#[test]
fn blueprint_owner_tab_close_stops_tab_owned_work() {
    assert!(!super::routes::owner_is_live(None, false, true));
    assert!(super::routes::owner_is_live(None, true, false));
}

#[test]
fn blueprint_owner_team_survives_creator_close_and_stops_with_team() {
    assert!(super::routes::owner_is_live(Some("team"), false, true));
    assert!(!super::routes::owner_is_live(Some("team"), true, false));
}

fn create(store: &mut Store, paused: bool, handlers: serde_json::Value) -> Sentinel {
    store.create(serde_json::from_value(json!({
        "name":"scheduler-result-watch",
        "probe":"status", "question":"state", "buckets":["done","failed"], "every":"10s", "timeout":"1h",
        "paused":paused, "handlers":handlers, "matches":{"done":"^done$"}
    })).unwrap(), owner(), false).unwrap()
}

#[test]
fn blueprint_owner_close_cancels_active_tab_owned_run_only() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(dir.path().join("sentinels.json"), clock);
    let tab_job = store
        .create(
            serde_json::from_value(json!({
                "name":"owner-close-tab", "probe":"status", "question":"state", "buckets":["done"], "every":"10s", "timeout":"1h", "matches":{"done":"^done$"}
            }))
            .unwrap(),
            Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "workspace".into(),
                cwd: "/tmp".into(),
            },
            false,
        )
        .unwrap();
    let team_job = create(&mut store, false, json!({}));
    let leases = store.due().unwrap();
    let tab_lease = leases
        .iter()
        .find(|lease| lease.job.id == tab_job.id)
        .unwrap();
    let team_lease = leases
        .iter()
        .find(|lease| lease.job.id == team_job.id)
        .unwrap();
    let conversion = tab_lease.cancelled.conversion_token();
    assert_eq!(store.cancel_tab_owned_runs("creator"), 1);
    assert!(tab_lease.is_cancelled());
    assert!(conversion.is_cancelled());
    assert!(!team_lease.is_cancelled());
}

fn execute(
    store: &Arc<Mutex<Store>>,
    lease: &super::store::Lease,
    text: &str,
    bucket: &str,
    confidence: f64,
    mut effect: impl FnMut(&Sentinel, &str, &Handler) -> Result<Status, String>,
) {
    super::scheduler::advance(
        store,
        lease,
        &super::probe::Output {
            bytes: text.as_bytes().to_vec(),
            text: text.into(),
            ..Default::default()
        },
        || {
            Classification::regex(Verdict {
                bucket: bucket.into(),
                confidence,
            })
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |job, transition, handler, _| effect(job, transition, handler).map(Into::into),
        |_, _, _| Ok(()),
    );
}

#[test]
fn blueprint_text_input_delivers_repeated_messages_without_hash_suppression() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1_000);
    let mut store = Store::new(dir.path().join("input.json"), clock);
    let job = store
        .create(
            serde_json::from_value(json!({
                "name":"input-send", "input":{"kind":"text","data":{"text":"initial"}},
                "question":"reply?", "buckets":["done"], "matches":{"done":".*"},
                "routes":{"done":"send:shell"}, "session":true
            }))
            .unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let store = Arc::new(Mutex::new(store));
    let deliveries = std::cell::RefCell::new(Vec::new());
    for _ in 0..3 {
        store.lock().wake_stage(&job.id);
        let lease = store.lock().due().unwrap().pop().unwrap();
        execute(&store, &lease, "same body", "done", 1.0, |_, _, handler| {
            assert_eq!(handler.action, Route::Send("shell".into()));
            deliveries.borrow_mut().push("same body");
            Ok(Status::Completed)
        });
        store.lock().release(&job.id);
    }
    assert_eq!(*deliveries.borrow(), ["same body"; 3]);
    assert_eq!(store.lock().get(&job.id).unwrap().transitions.len(), 3);
    assert!(store.lock().due().unwrap().is_empty());
}

#[test]
fn blueprint_session_exchanges_bypass_budgets_but_stop_at_safety_and_rate_limits() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1_000);
    let mut store = Store::new(dir.path().join("session.json"), clock.clone());
    let mut params: super::store::Create = serde_json::from_value(json!({
        "name":"conversation", "input":{"kind":"text","data":{"text":"initial"}},
        "question":"reply?", "buckets":["done"], "matches":{"done":".*"},
        "routes":{"done":"send:shell"}, "session":true, "max_transitions":1,"max_actions":1
    }))
    .unwrap();
    params.session = true;
    let job = store.create(params, owner(), false).unwrap();
    let store = Arc::new(Mutex::new(store));
    for index in 0..121 {
        clock.set_ms(1_000 + index * 1_001);
        store.lock().wake_stage(&job.id);
        let lease = store.lock().due().unwrap().pop().unwrap();
        execute(&store, &lease, "repeat", "done", 1.0, |_, _, _| {
            Ok(Status::Completed)
        });
        store.lock().release(&job.id);
    }
    let mut live = store.lock().get(&job.id).unwrap().clone();
    assert!(!live.machine.stopped);
    assert_eq!(live.transitions.len(), 121);
    live.session = false;
    assert!(super::store::transition_budget_spent(&live));
    live.session = true;
    assert!(!super::store::transition_budget_spent(&live));
    let transition = live.transitions[0].clone();
    live.transitions = vec![transition; codeterm_types::blueprint_terminal::SESSION_SAFETY_LIMIT];
    assert!(super::store::transition_budget_spent(&live));
    {
        let mut store = store.lock();
        let current = store
            .jobs
            .iter_mut()
            .find(|current| current.id == job.id)
            .unwrap();
        for transition in &mut current.transitions {
            transition.accepted_at = 122_121;
        }
    }
    clock.set_ms(122_121);
    let current = store.lock().get(&job.id).unwrap().clone();
    assert!(!store
        .lock()
        .accept_transition(&current, "done", "rate-overflow")
        .unwrap());
    let stopped = store.lock().get(&job.id).unwrap().clone();
    assert!(stopped.machine.stopped);
    assert_eq!(stopped.last_reason.as_deref(), Some("session_rate_limit"));
}

#[test]
fn sentinel_scheduler_graph_selected_child_activates_once_and_reaches_accepted_result() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        clock,
    )));
    let child = create(
        &mut store.lock(),
        true,
        json!({"done":[{"id":"end","type":"stop","parameters":{}}]}),
    );
    let other = create(&mut store.lock(), true, json!({}));
    let parent = create(
        &mut store.lock(),
        false,
        json!({"done":[{"id":"branch","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let lease = store.lock().due().unwrap().remove(0);
    assert_eq!(lease.job.id, parent.id);
    execute(
        &store,
        &lease,
        "done",
        "done",
        1.0,
        |job, transition, handler| {
            let Route::Sentinel(target) = &handler.action else {
                panic!("wrong action")
            };
            assert!(store.lock().activate(job, transition, target).unwrap());
            assert!(!store.lock().activate(job, transition, target).unwrap());
            Ok(Status::Completed)
        },
    );
    store.lock().release(&parent.id);
    assert!(store.lock().get(&other.id).unwrap().machine.paused);
    let child_lease = store.lock().due().unwrap().remove(0);
    assert_eq!(child_lease.job.id, child.id);
    execute(
        &store,
        &child_lease,
        "done",
        "done",
        1.0,
        |job, _, handler| {
            assert_eq!(handler.action, Route::Stop);
            store.lock().stop(&job.id, "stop_handler").unwrap();
            Ok(Status::Completed)
        },
    );
    let current = store.lock();
    let end = current.get(&child.id).unwrap();
    assert!(end.machine.stopped);
    assert_eq!(end.machine.last_bucket.as_deref(), Some("done"));
    assert_eq!(end.transitions.len(), 1);
    assert_eq!(end.transitions[0].handlers[0].status, Status::Completed);
}

#[test]
fn sentinel_scheduler_graph_weak_other_and_unchanged_never_activate() {
    for (bucket, confidence) in [("done", 0.79), ("other", 1.0), ("unknown", 1.0)] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(Store::new(
            dir.path().join("sentinels.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        )));
        let child = create(&mut store.lock(), true, json!({}));
        create(
            &mut store.lock(),
            false,
            json!({"done":[{"id":"branch","type":"blueprint","parameters":{"target":child.id}}]}),
        );
        let lease = store.lock().due().unwrap().remove(0);
        execute(&store, &lease, "same", bucket, confidence, |_, _, _| {
            panic!("weak verdict cannot act")
        });
        let mut repeated = lease.job.clone();
        repeated.machine = store.lock().get(&repeated.id).unwrap().machine.clone();
        let lease = super::store::Lease {
            job: repeated,
            cancelled: lease.cancelled,
            event: None,
        };
        super::scheduler::advance(
            &store,
            &lease,
            &super::probe::Output {
                bytes: b"same".to_vec(),
                text: "same".into(),
                ..Default::default()
            },
            || panic!("unchanged cannot classify"),
            |_, _, _| panic!("unchanged cannot claim"),
            |_, _, _, _| panic!("unchanged cannot act"),
            |_, _, _| Ok(()),
        );
        assert!(store.lock().get(&child.id).unwrap().machine.paused);
    }
}

#[test]
fn sentinel_scheduler_graph_partial_failure_preserves_order_and_independent_results() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    )));
    let parent = create(
        &mut store.lock(),
        false,
        json!({"done":[
            {"id":"first","type":"notify","parameters":{}}, {"id":"second","type":"notify","parameters":{}}, {"id":"third","type":"notify","parameters":{}}
        ]}),
    );
    let lease = store.lock().due().unwrap().remove(0);
    let mut order = Vec::new();
    execute(&store, &lease, "done", "done", 1.0, |_, _, handler| {
        order.push(handler.id.clone());
        if handler.id == "second" {
            Err("ordinary refusal".into())
        } else {
            Ok(Status::Completed)
        }
    });
    assert_eq!(order, ["first", "second", "third"]);
    let current = store.lock();
    let records = &current.get(&parent.id).unwrap().transitions[0].handlers;
    assert_eq!(
        records.iter().map(|r| &r.status).collect::<Vec<_>>(),
        [&Status::Completed, &Status::Failed, &Status::Completed]
    );
}

#[test]
fn sentinel_scheduler_graph_stop_and_team_cleanup_fence_remaining_claims() {
    for cleanup in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(Store::new(
            dir.path().join("sentinels.json"),
            crate::cron::clock::MockClock::starting_at(1000),
        )));
        let parent = create(
            &mut store.lock(),
            false,
            json!({"done":[{"id":"first","type":"notify","parameters":{}},{"id":"second","type":"notify","parameters":{}}]}),
        );
        let lease = store.lock().due().unwrap().remove(0);
        let mut calls = 0;
        execute(&store, &lease, "done", "done", 1.0, |job, _, _| {
            calls += 1;
            if cleanup {
                store.lock().remove_team("team").unwrap();
            } else {
                store.lock().stop(&job.id, "external_stop").unwrap();
            }
            Ok(Status::Completed)
        });
        assert_eq!(calls, 1);
        let current = store.lock();
        if cleanup {
            assert!(current.get(&parent.id).is_none());
        } else {
            assert_eq!(
                current.get(&parent.id).unwrap().transitions[0].handlers[1].status,
                Status::Skipped
            );
        }
    }
}

#[test]
fn sentinel_scheduler_graph_until_finishes_its_claimed_batch_and_stop_skips_tail() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    )));
    let mut params = json!({"name":"scheduler-stop-watch","probe":"status","question":"state","buckets":["done"],"every":"10s","timeout":"1h","until":"done","matches":{"done":"done"},"handlers":{"done":[{"id":"first","type":"notify","parameters":{}},{"id":"end","type":"stop","parameters":{}},{"id":"tail","type":"notify","parameters":{}}]}});
    params["paused"] = json!(false);
    let parent = store
        .lock()
        .create(serde_json::from_value(params).unwrap(), owner(), false)
        .unwrap();
    let lease = store.lock().due().unwrap().remove(0);
    let mut calls = Vec::new();
    execute(&store, &lease, "done", "done", 1.0, |job, _, handler| {
        calls.push(handler.id.clone());
        if handler.action == Route::Stop {
            store.lock().stop(&job.id, "stop_handler").unwrap();
        }
        Ok(Status::Completed)
    });
    assert_eq!(calls, ["first", "end"]);
    assert_eq!(
        store.lock().get(&parent.id).unwrap().transitions[0].handlers[2].status,
        Status::Skipped
    );
}

#[test]
fn sentinel_scheduler_graph_revision_change_during_classification_refuses_batch() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    )));
    let job = create(&mut store.lock(), false, json!({}));
    let lease = store.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &store,
        &lease,
        &super::probe::Output {
            bytes: b"done".to_vec(),
            text: "done".into(),
            ..Default::default()
        },
        || {
            store.lock().stop(&job.id, "external_stop").unwrap();
            Classification::regex(Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |_, _, _| panic!("stale revision cannot claim"),
        |_, _, _, _| panic!("stale revision cannot act"),
        |_, _, _| Ok(()),
    );
    assert!(store.lock().get(&job.id).unwrap().transitions.is_empty());
}

#[test]
fn sentinel_scheduler_graph_stop_after_claim_leaves_unconfirmed_and_skips_tail() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    )));
    let job = create(
        &mut store.lock(),
        false,
        json!({"done":[{"id":"first","type":"notify","parameters":{}},{"id":"tail","type":"notify","parameters":{}}]}),
    );
    let lease = store.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &store,
        &lease,
        &super::probe::Output {
            bytes: b"done".to_vec(),
            text: "done".into(),
            ..Default::default()
        },
        || {
            Classification::regex(Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| {
            let claim = store.lock().claim_handler(job, transition, handler)?;
            store.lock().stop(&job.id, "external_stop")?;
            Ok(claim)
        },
        |_, _, _, _| panic!("cancelled claim cannot act"),
        |_, _, _| Ok(()),
    );
    let current = store.lock();
    let handlers = &current.get(&job.id).unwrap().transitions[0].handlers;
    assert_eq!(handlers[0].status, Status::Unconfirmed);
    assert_eq!(handlers[1].status, Status::Skipped);
}

#[test]
fn sentinel_scheduler_graph_two_parents_join_running_child_without_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(path.clone(), clock.clone())));
    let child = create(&mut store.lock(), true, json!({}));
    let first = create(
        &mut store.lock(),
        false,
        json!({"done":[{"id":"join","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let second = create(
        &mut store.lock(),
        false,
        json!({"done":[{"id":"join","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let leases = store.lock().due().unwrap();
    let one = leases.iter().find(|l| l.job.id == first.id).unwrap();
    execute(&store, one, "first", "done", 1.0, |job, t, h| {
        let Route::Sentinel(id) = &h.action else {
            panic!()
        };
        assert!(store.lock().activate(job, t, id).unwrap());
        Ok(Status::Completed)
    });
    let child_lease = store.lock().due().unwrap().remove(0);
    assert_eq!(child_lease.job.id, child.id);
    let before = serde_json::to_value(store.lock().get(&child.id).unwrap()).unwrap();
    let two = leases.iter().find(|l| l.job.id == second.id).unwrap();
    execute(&store, two, "second", "done", 1.0, |job, t, h| {
        let Route::Sentinel(id) = &h.action else {
            panic!()
        };
        assert!(!store.lock().activate(job, t, id).unwrap());
        Ok(Status::Noop)
    });
    let after = serde_json::to_value(store.lock().get(&child.id).unwrap()).unwrap();
    assert_eq!(before, after);
    assert!(!child_lease
        .cancelled
        .load(std::sync::atomic::Ordering::SeqCst));
    execute(
        &store,
        &child_lease,
        "child-done",
        "done",
        1.0,
        |_, _, _| Ok(Status::Completed),
    );
    let restored = Store::new(path, clock);
    assert_eq!(
        restored
            .get(&child.id)
            .unwrap()
            .machine
            .last_bucket
            .as_deref(),
        Some("done")
    );
    assert_eq!(
        restored.get(&second.id).unwrap().transitions[0].handlers[0].status,
        Status::Noop
    );
}

#[test]
fn sentinel_scheduler_graph_action_budget_stops_before_next_effect() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    )));
    let params = json!({"name":"scheduler-budget-watch","probe":"status","question":"state","buckets":["done"],"every":"10s","timeout":"1h","matches":{"done":"done"},"max_actions":1,"handlers":{"done":[{"id":"first","type":"notify","parameters":{}},{"id":"tail","type":"notify","parameters":{}}]}});
    let job = store
        .lock()
        .create(serde_json::from_value(params).unwrap(), owner(), false)
        .unwrap();
    let lease = store.lock().due().unwrap().remove(0);
    let mut calls = 0;
    execute(&store, &lease, "done", "done", 1.0, |_, _, _| {
        calls += 1;
        Ok(Status::Completed)
    });
    assert_eq!(calls, 1);
    let current = store.lock();
    let job = current.get(&job.id).unwrap();
    assert_eq!(job.last_reason.as_deref(), Some("action_budget_exhausted"));
    assert_eq!(job.transitions[0].handlers[1].status, Status::Skipped);
}

#[test]
fn blueprint_delivery_route_claim_journal_failure_finishes_failed_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(path.clone(), clock.clone())));
    let job = create(
        &mut store.lock(),
        false,
        json!({"done":[{"id":"attempt","type":"notify","parameters":{}}]}),
    );
    let lease = store.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &store,
        &lease,
        &super::probe::Output {
            bytes: b"done".to_vec(),
            text: "done".into(),
            ..Default::default()
        },
        || {
            Classification::regex(Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |_, _, _, _| panic!("journal refusal prevents effect"),
        |_, _, stage| {
            if matches!(stage, super::provenance::Stage::RouteClaim) {
                Err(super::scheduler::known_pre_effect_journal_refusal(
                    "route claim rejected before effect",
                ))
            } else {
                Ok(())
            }
        },
    );
    drop(store);
    clock.advance_ms(10_000);
    let restored = Arc::new(Mutex::new(Store::new(path, clock)));
    let due = restored.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &restored,
        &due,
        &super::probe::Output {
            bytes: b"done".to_vec(),
            text: "done".into(),
            ..Default::default()
        },
        || panic!("persisted observation must not reclassify"),
        |_, _, _| panic!("persisted claim must not replay"),
        |_, _, _, _| panic!("persisted effect must not replay"),
        |_, _, _| Ok(()),
    );
    let current = restored.lock();
    let records = &current.get(&job.id).unwrap().transitions;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].handlers[0].status, Status::Failed);
    assert_eq!(
        records[0].handlers[0].reason.as_deref(),
        Some("journal_route_claim_failed")
    );
}

#[test]
fn blueprint_delivery_ambiguous_route_claim_journal_error_stays_unconfirmed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(path.clone(), clock.clone())));
    let job = create(
        &mut store.lock(),
        false,
        json!({"done":[{"id":"attempt","type":"notify","parameters":{}}]}),
    );
    let lease = store.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &store,
        &lease,
        &super::probe::Output {
            bytes: b"done".to_vec(),
            text: "done".into(),
            ..Default::default()
        },
        || {
            Classification::regex(Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |_, _, _, _| panic!("ambiguous route claim must not run the effect"),
        |_, _, stage| {
            if matches!(stage, super::provenance::Stage::RouteClaim) {
                Err(std::io::Error::other("interrupted after durable claim"))
            } else {
                Ok(())
            }
        },
    );
    let current = store.lock();
    let claim = &current.get(&job.id).unwrap().transitions[0].handlers[0];
    assert_eq!(claim.status, Status::Unconfirmed);
    assert_eq!(
        claim.reason.as_deref(),
        Some("route_claim_journal_unconfirmed")
    );
}

#[test]
fn blueprint_delivery_rearm_on_change_claims_executes_and_completes() {
    use super::provenance::Stage;

    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("blueprints.json"),
        clock.clone(),
    )));
    let job = store
        .lock()
        .create(
            serde_json::from_value(json!({
                "name":"rearm-delivery-watch",
                "probe":"status",
                "question":"state",
                "buckets":["done"],
                "every":"10s",
                "timeout":"1h",
                "matches":{"done":".*"},
                "rearm_on_change":["done"],
                "handlers":{"done":[{"id":"notice","type":"notify","parameters":{}}]}
            }))
            .unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let mut stages = Vec::new();
    for output in ["first", "second"] {
        let lease = store.lock().due().unwrap().remove(0);
        super::scheduler::advance(
            &store,
            &lease,
            &super::probe::Output {
                bytes: output.as_bytes().to_vec(),
                text: output.into(),
                ..Default::default()
            },
            || {
                Classification::regex(Verdict {
                    bucket: "done".into(),
                    confidence: 1.0,
                })
            },
            |job, transition, handler| store.lock().claim_handler(job, transition, handler),
            |_, transition, _, _| {
                Ok(super::handlers::Effect {
                    status: Status::Completed,
                    execution_id: Some(format!("execution-{transition}")),
                    command_result: None,
                    reason: None,
                })
            },
            |_, _, stage| {
                stages.push(stage);
                Ok(())
            },
        );
        store.lock().release(&job.id);
        clock.advance_ms(10_000);
    }
    assert_eq!(
        stages,
        [
            Stage::Decision,
            Stage::RouteClaim,
            Stage::RouteCompleted,
            Stage::Decision,
            Stage::RouteClaim,
            Stage::RouteCompleted,
        ]
    );
    let current = store.lock();
    let job = current.get(&job.id).unwrap();
    assert_eq!(job.transitions.len(), 2);
    assert!(job.transitions.iter().all(|transition| {
        transition.handlers[0].execution_id.is_some()
            && transition.handlers[0].status == Status::Completed
    }));
}
