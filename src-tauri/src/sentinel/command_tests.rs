use super::store::{Owner, Store};
use serde_json::{json, Value};

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

fn definition(kind: &str, parameters: Value) -> Value {
    json!({
        "name":"command-result-watch",
        "probe":"status", "question":"state?", "buckets":["done"],
        "every":"10s", "matches":{"done":"done"}, "timeout":"1h",
        "handlers":{"done":[{"id":"action", "type":kind, "parameters":parameters}]}
    })
}

fn create(
    store: &mut Store,
    kind: &str,
    parameters: Value,
) -> Result<super::store::Sentinel, String> {
    serde_json::from_value(definition(kind, parameters))
        .map_err(|error| error.to_string())
        .and_then(|params| store.create(params, owner(), false))
}

#[test]
fn sentinel_command_existing_notify_creation_positive() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create(&mut store, "notify", json!({})).unwrap();
    assert_eq!(job.routes["done"][0].id, "action");
}

#[test]
fn sentinel_command_fixed_definition_admitted_through_create() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let result = create(
        &mut store,
        "command",
        json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}),
    );
    assert!(
        result.is_ok(),
        "fixed creator command must be admitted: {result:?}"
    );
}

#[test]
fn sentinel_command_fixed_definition_persists_creator_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let result = create(
        &mut store,
        "command",
        json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}),
    );
    assert!(
        result.is_ok(),
        "fixed command creation must reach durable storage: {result:?}"
    );
    let job = result.unwrap();
    let restored = Store::new(path, clock);
    assert_eq!(restored.get(&job.id).unwrap().cwd, owner().cwd);
}

use super::command::{self, Status};
use super::handlers::Status as HandlerStatus;
use super::machine::{Route, Verdict};
use super::probe::{
    self,
    cancellation::{Cancellation, Wake},
    process::{ProbeFailure, ResourceKind},
};
use super::provenance::Classification;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

fn parameters() -> command::Parameters {
    command::Parameters {
        command: "printf fixed-secret-marker".into(),
        label: "Marker".into(),
        timeout_ms: 1000,
    }
}

fn command_handlers() -> Value {
    json!({"done":[
        {"id":"command", "type":"command", "parameters":parameters()},
        {"id":"notice", "type":"notify", "parameters":{}}
    ]})
}

fn command_store(path: std::path::PathBuf) -> Arc<Mutex<Store>> {
    let mut store = Store::new(path, crate::cron::clock::MockClock::starting_at(1000));
    let mut definition = definition("notify", json!({}));
    definition["handlers"] = command_handlers();
    store
        .create(serde_json::from_value(definition).unwrap(), owner(), false)
        .unwrap();
    Arc::new(Mutex::new(store))
}

fn advance(
    store: &Arc<Mutex<Store>>,
    lease: &super::store::Lease,
    launch: impl FnMut(
        &super::store::Sentinel,
        &str,
        &super::handlers::Handler,
    ) -> Result<super::handlers::Effect, String>,
) {
    super::scheduler::advance(
        store,
        lease,
        &probe::fixture_output(0, b"done", b""),
        || {
            Classification::regex(Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        {
            let mut launch = launch;
            move |job, transition, handler, _| launch(job, transition, handler)
        },
        |_, _, _| Ok(()),
    );
}

#[test]
fn sentinel_command_typed_native_exit_and_capture_accounting_preserve_bytes() {
    for code in [0, 7] {
        let output = probe::fixture_output(code, b"exit=0\nraw-secret", b"error-secret");
        let mut expected = code.to_le_bytes().to_vec();
        expected.extend_from_slice(&(17u64).to_le_bytes());
        expected.extend_from_slice(b"exit=0\nraw-secret");
        expected.extend_from_slice(b"error-secret");
        assert_eq!(output.bytes, expected);
        assert_eq!(
            output.text,
            format!("exit={code}\nstdout:\nexit=0\nraw-secret\nstderr:\nerror-secret")
        );
        let effect = command::execute_with(
            &parameters(),
            Duration::from_secs(2),
            || 1000,
            |_| Ok(output),
        );
        assert!(effect.execution_id.is_some());
        let metadata = effect.command_result.unwrap();
        assert_eq!(metadata.exit_code, Some(code));
        assert_eq!(
            metadata.status,
            if code == 0 {
                Status::Success
            } else {
                Status::Nonzero
            }
        );
        assert!(metadata.resources_confirmed);
        assert_eq!(metadata.stdout.unwrap().total_bytes, 17);
        assert_eq!(
            effect.status,
            if code == 0 {
                HandlerStatus::Completed
            } else {
                HandlerStatus::Failed
            }
        );
    }
    let output = probe::fixture_output(0, &vec![b'x'; 100_000], b"e");
    assert_eq!(output.stdout.total_bytes, 100_000);
    assert_eq!(output.stdout.retained_bytes, 32 * 1024);
    assert!(output.stdout.truncated);
}

#[test]
fn sentinel_command_failure_states_keep_actual_exit_unknown_and_cleanup_distinct() {
    for (primary, expected) in [
        ("probe_spawn_failed", Status::Error),
        ("probe_cancel_wake_failed", Status::Error),
        ("probe_capture_or_completion_failed", Status::Error),
        ("probe_timeout", Status::Deadline),
        ("probe_cancelled", Status::Cancelled),
    ] {
        let effect = command::execute_with(
            &parameters(),
            Duration::from_secs(1),
            || 1234,
            |_| Err(ProbeFailure::new(primary)),
        );
        let metadata = effect.command_result.unwrap();
        assert_eq!(metadata.status, expected);
        assert_eq!(metadata.exit_code, None);
        assert_eq!(metadata.stdout, None);
        assert!(metadata.resources_confirmed);
        assert_eq!(metadata.primary.as_deref(), Some(primary));
    }
    let effect = command::execute_with(
        &parameters(),
        Duration::from_secs(1),
        || 1234,
        |_| {
            let mut failure = ProbeFailure::new("probe_cancelled");
            failure.unconfirmed(ResourceKind::Capture);
            Err(failure)
        },
    );
    assert_eq!(effect.status, HandlerStatus::Unconfirmed);
    let metadata = effect.command_result.unwrap();
    assert_eq!(metadata.status, Status::Unconfirmed);
    assert_eq!(metadata.primary.as_deref(), Some("probe_cancelled"));
    assert!(!metadata.resources_confirmed);
    let actual = probe::fixture_output(7, b"known", b"err");
    let effect = command::execute_with(
        &parameters(),
        Duration::from_secs(1),
        || 1234,
        |_| {
            let mut failure = ProbeFailure::new("probe_owner_release_failed").with_output(&actual);
            failure.unconfirmed(ResourceKind::Capture);
            Err(failure)
        },
    );
    let metadata = effect.command_result.unwrap();
    assert_eq!(metadata.status, Status::Unconfirmed);
    assert_eq!(metadata.exit_code, Some(7));
    assert_eq!(metadata.stdout.unwrap().total_bytes, 5);
    assert_eq!(metadata.stderr.unwrap().total_bytes, 3);
}

#[test]
fn sentinel_command_remaining_deadline_is_passed_once_to_existing_runner() {
    command::execute_with(
        &parameters(),
        Duration::from_millis(321),
        || 1000,
        |limit| {
            assert_eq!(limit, Duration::from_millis(321));
            Err(ProbeFailure::new("probe_timeout"))
        },
    );
}

#[test]
fn sentinel_command_claim_before_launch_and_independent_nonzero_survive_reload_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let store = command_store(path.clone());
    let lease = store.lock().due().unwrap().remove(0);
    let mut launches = 0;
    advance(&store, &lease, |job, _, handler| {
        if let Route::Command(parameters) = &handler.action {
            let current = store.lock();
            let claimed = &current.get(&job.id).unwrap().transitions[0].handlers[0];
            assert_eq!(claimed.status, HandlerStatus::Unconfirmed);
            assert!(claimed.claim_id.is_some());
            drop(current);
            launches += 1;
            Ok(command::execute_with(
                parameters,
                Duration::from_secs(1),
                || 1000,
                |_| Ok(probe::fixture_output(9, b"exit=0", b"private-output")),
            ))
        } else {
            Ok(HandlerStatus::Completed.into())
        }
    });
    assert_eq!(launches, 1);
    let before = store.lock().get(&lease.job.id).unwrap().clone();
    assert_eq!(
        before.transitions[0].handlers[0].status,
        HandlerStatus::Failed
    );
    assert_eq!(
        before.transitions[0].handlers[1].status,
        HandlerStatus::Completed
    );
    assert_eq!(
        before.transitions[0].handlers[0]
            .command_result
            .as_ref()
            .unwrap()
            .status,
        Status::Nonzero
    );
    drop(store);
    let mut restored = Store::new(path, crate::cron::clock::MockClock::starting_at(1000));
    assert_eq!(
        restored.get(&lease.job.id).unwrap().transitions[0].handlers[0].execution_id,
        before.transitions[0].handlers[0].execution_id
    );
    for handler in &before.routes["done"] {
        assert!(restored
            .claim_handler(&before, &before.transitions[0].id, handler)
            .unwrap()
            .is_none());
    }
}

#[test]
fn sentinel_command_claim_persistence_failure_never_launches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let store = command_store(path.clone());
    let lease = store.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &store,
        &lease,
        &probe::fixture_output(0, b"done", b""),
        || {
            Classification::regex(Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| {
            std::fs::rename(&path, dir.path().join("before.json")).unwrap();
            std::fs::create_dir(&path).unwrap();
            store.lock().claim_handler(job, transition, handler)
        },
        |_, _, _, _| panic!("failed durable claim must never launch"),
        |_, _, _| Ok(()),
    );
    assert!(store.lock().get(&lease.job.id).unwrap().transitions[0]
        .handlers
        .iter()
        .all(|result| result.claim_id.is_none()));
}

#[test]
fn sentinel_command_unconfirmed_cleanup_fences_siblings_and_retains_result() {
    let dir = tempfile::tempdir().unwrap();
    let store = command_store(dir.path().join("sentinels.json"));
    let lease = store.lock().due().unwrap().remove(0);
    advance(&store, &lease, |_, _, handler| {
        let Route::Command(parameters) = &handler.action else {
            panic!("unsafe continuation");
        };
        Ok(command::execute_with(
            parameters,
            Duration::from_secs(1),
            || 1000,
            |_| {
                let mut failure = ProbeFailure::new("probe_completion_unconfirmed");
                failure.unconfirmed(ResourceKind::Child);
                Err(failure)
            },
        ))
    });
    let current = store.lock();
    let job = current.get(&lease.job.id).unwrap();
    assert!(job.machine.stopped);
    assert_eq!(
        job.transitions[0].handlers[0].status,
        HandlerStatus::Unconfirmed
    );
    assert_eq!(
        job.transitions[0].handlers[1].status,
        HandlerStatus::Skipped
    );
}

#[test]
fn sentinel_command_late_natural_success_preserved_without_rearming_stopped_source() {
    let dir = tempfile::tempdir().unwrap();
    let store = command_store(dir.path().join("sentinels.json"));
    let lease = store.lock().due().unwrap().remove(0);
    advance(&store, &lease, |job, _, handler| {
        let Route::Command(parameters) = &handler.action else {
            panic!("revoked source cannot continue");
        };
        Ok(command::execute_with(
            parameters,
            Duration::from_secs(1),
            || 1000,
            |_| {
                store.lock().stop(&job.id, "revoked").unwrap();
                assert!(lease.cancelled.load(std::sync::atomic::Ordering::SeqCst));
                Ok(probe::fixture_output(
                    0,
                    b"side effect already completed",
                    b"",
                ))
            },
        ))
    });
    let current = store.lock();
    let job = current.get(&lease.job.id).unwrap();
    assert!(job.machine.stopped);
    assert_eq!(
        job.transitions[0].handlers[0].status,
        HandlerStatus::Completed
    );
    assert_eq!(
        job.transitions[0].handlers[1].status,
        HandlerStatus::Skipped
    );
}

struct CountWake(std::sync::atomic::AtomicUsize);
impl Wake for CountWake {
    fn wake(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn sentinel_command_cancellation_registration_race_latches_once_and_unregisters() {
    for before in [true, false] {
        let signal = Cancellation::default();
        let wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
        if before {
            signal.cancel();
        }
        let registration = signal.register(wake.clone()).unwrap();
        if !before {
            signal.cancel();
        }
        signal.cancel();
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(signal.register(wake.clone()).is_err());
        drop(registration);
        let other = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
        let _registration = signal.register(other.clone()).unwrap();
        assert_eq!(other.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    let signal = Cancellation::default();
    let wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
    drop(signal.register(wake.clone()).unwrap());
    signal.cancel();
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn sentinel_command_definition_rejects_invalid_text_bounds_options_and_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    for parameters in [
        json!({"command":"", "label":"a", "timeout_ms":1}),
        json!({"command":"a\u{0000}b", "label":"a", "timeout_ms":1}),
        json!({"command":"x".repeat(8193), "label":"a", "timeout_ms":1}),
        json!({"command":"x", "label":"a".repeat(257), "timeout_ms":1}),
        json!({"command":"x", "label":"a", "timeout_ms":0}),
        json!({"command":"x", "label":"a", "timeout_ms":10_001}),
        json!({"command":"x", "label":"a", "timeout_ms":1, "target":"tab"}),
        json!({"command":"x", "label":"a", "timeout_ms":1, "cwd":"/tmp"}),
        json!({"command":"x", "label":"a", "timeout_ms":1, "env":{}}),
    ] {
        assert!(create(&mut store, "command", parameters).is_err());
    }
    assert!(create(
        &mut store,
        "notify",
        json!({"command":"x", "label":"a", "timeout_ms":1})
    )
    .is_err());
}

#[test]
fn sentinel_command_graph_metadata_is_safe_and_preserves_actual_execution_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    )));
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.snapshot().active_workspace_id.unwrap();
    let creator = mux.create_tab(&workspace).unwrap();
    let owner = Owner {
        creator_tab_id: creator,
        workspace_id: workspace.clone(),
        team_id: None,
        cwd: "/tmp".into(),
    };
    let mut definition = definition("command", serde_json::to_value(parameters()).unwrap());
    definition["name"] = json!("Named command scenario");
    let mut job = store
        .lock()
        .prepare(serde_json::from_value(definition).unwrap(), owner, false)
        .unwrap();
    job.agent_authored = true;
    let job = store.lock().insert(job).unwrap();
    let lease = store.lock().due().unwrap().remove(0);
    advance(&store, &lease, |_, _, handler| {
        let Route::Command(parameters) = &handler.action else {
            panic!("wrong command");
        };
        Ok(command::execute_with(
            parameters,
            Duration::from_secs(1),
            || 1000,
            |_| {
                Ok(probe::fixture_output(
                    0,
                    b"raw-private-stdout",
                    b"raw-private-stderr",
                ))
            },
        ))
    });
    let store = store.lock();
    let snapshot = super::graph::project(
        &workspace,
        None,
        store.list(),
        &mux,
        &std::collections::BTreeMap::new(),
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(snapshot.scenarios.len(), 1);
    assert_eq!(snapshot.scenarios[0].id, job.id);
    assert!(snapshot.edges.is_empty());
    let projected = &snapshot.nodes[0]["outcomes"][0]["handlers"][0];
    assert_eq!(projected["target"], Value::Null);
    assert_eq!(projected["label"], "Marker");
    assert_eq!(projected["timeout_ms"], 1000);
    assert_eq!(projected["last_result"]["command_result"]["exit_code"], 0);
    assert_eq!(
        projected["last_result"]["execution_id"],
        serde_json::to_value(&store.get(&job.id).unwrap().transitions[0].handlers[0].execution_id)
            .unwrap()
    );
    assert_eq!(
        snapshot.nodes[0]["last_result"]["handlers"][0]["command_result"],
        projected["last_result"]["command_result"]
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    // The owner sees probe and command source in the graph; captured output and cwd stay private.
    for private in ["raw-private-stdout", "raw-private-stderr", "\"cwd\""] {
        assert!(!encoded.contains(private), "graph leaked {private}");
    }
}

#[test]
fn sentinel_command_cancel_register_concurrently_delivers_once() {
    for _ in 0..32 {
        let signal = Cancellation::default();
        let wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_signal = signal.clone();
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            worker_signal.cancel();
        });
        barrier.wait();
        let _registration = signal.register(wake.clone()).unwrap();
        worker.join().unwrap();
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

#[test]
fn sentinel_command_scope_expiry_team_cleanup_and_stop_wake_exact_source() {
    for removal in ["stop", "team", "expiry"] {
        let dir = tempfile::tempdir().unwrap();
        let clock = crate::cron::clock::MockClock::starting_at(1000);
        let mut store = Store::new(dir.path().join("sentinels.json"), clock.clone());
        let first = create(
            &mut store,
            "command",
            serde_json::to_value(parameters()).unwrap(),
        )
        .unwrap();
        let mut other_owner = owner();
        other_owner.team_id = Some("other-team".into());
        let mut other_definition = definition("notify", json!({}));
        other_definition["timeout"] = json!("2h");
        let other = store
            .create(
                serde_json::from_value(other_definition).unwrap(),
                other_owner,
                false,
            )
            .unwrap();
        let leases = store.due().unwrap();
        let lease = leases
            .iter()
            .find(|lease| lease.job.id == first.id)
            .unwrap();
        let unrelated = leases
            .iter()
            .find(|lease| lease.job.id == other.id)
            .unwrap();
        let wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
        let unrelated_wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
        let _registration = lease.cancelled.register(wake.clone()).unwrap();
        let _unrelated_registration = unrelated
            .cancelled
            .register(unrelated_wake.clone())
            .unwrap();
        match removal {
            "stop" => {
                store.stop(&first.id, "explicit").unwrap();
            }
            "team" => {
                store.remove_team("team").unwrap();
            }
            "expiry" => {
                clock.advance_ms(3_600_000);
                store.due().unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            unrelated_wake.0.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }
}

#[test]
fn sentinel_command_shutdown_subscription_closes_source_admission_and_wakes_scheduler() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create(
        &mut store,
        "command",
        serde_json::to_value(parameters()).unwrap(),
    )
    .unwrap();
    let lease = store.due().unwrap().remove(0);
    let wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
    let _registration = lease.cancelled.register(wake.clone()).unwrap();
    let handle = super::scheduler::fixture_handle_without_app(store);
    let latch = crate::crash::shutdown::ShutdownLatch::default();
    let _subscription = super::scheduler::fixture_shutdown(&handle, &latch);
    assert_eq!(
        super::scheduler::fixture_shutdown_status(&handle),
        (false, 0)
    );
    latch.arm(crate::crash::lifecycle::ShutdownReason::UserExit);
    assert_eq!(
        super::scheduler::fixture_shutdown_status(&handle),
        (true, 1)
    );
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(handle.store.lock().due().is_err());
    assert!(create(
        &mut handle.store.lock(),
        "command",
        serde_json::to_value(parameters()).unwrap()
    )
    .is_err());
    assert!(handle.store.lock().get(&job.id).is_some());
    latch.arm(crate::crash::lifecycle::ShutdownReason::Signal);
    assert_eq!(
        super::scheduler::fixture_shutdown_status(&handle),
        (true, 1)
    );
}

#[test]
fn sentinel_command_revision_or_owner_change_after_claim_refuses_launch() {
    for change in ["revision", "creator", "team", "workspace"] {
        let dir = tempfile::tempdir().unwrap();
        let store = command_store(dir.path().join("sentinels.json"));
        let lease = store.lock().due().unwrap().remove(0);
        super::scheduler::advance(
            &store,
            &lease,
            &probe::fixture_output(0, b"done", b""),
            || {
                Classification::regex(Verdict {
                    bucket: "done".into(),
                    confidence: 1.0,
                })
            },
            |job, transition, handler| {
                let claim = store.lock().claim_handler(job, transition, handler)?;
                let mut store = store.lock();
                let current = store
                    .jobs
                    .iter_mut()
                    .find(|current| current.id == job.id)
                    .unwrap();
                match change {
                    "revision" => current.revision += 1,
                    "creator" => current.creator_tab_id = "revoked".into(),
                    "team" => current.team_id = None,
                    "workspace" => current.workspace_id = "revoked".into(),
                    _ => unreachable!(),
                }
                Ok(claim)
            },
            |_, _, _, _| panic!("changed owner/revision must refuse launch"),
            |_, _, _| Ok(()),
        );
        let current = store.lock();
        let records = &current.get(&lease.job.id).unwrap().transitions[0].handlers;
        assert_eq!(records[0].status, HandlerStatus::Unconfirmed);
        assert!(records[0].command_result.is_none());
        assert_eq!(records[1].status, HandlerStatus::Skipped);
    }
}

#[test]
fn sentinel_command_creation_reaches_independent_persisted_execution_seam() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let created = create(
        &mut store,
        "command",
        json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}),
    );
    assert!(
        created.is_ok(),
        "fixed command must reach the settled executor seam: {created:?}"
    );
    let job = created.unwrap();
    let lease = store.due().unwrap().remove(0);
    let store = std::sync::Arc::new(parking_lot::Mutex::new(store));
    let owner = std::sync::Arc::new(super::probe::owner::ProbeResourceOwner::new());
    #[cfg(unix)]
    let producer = "printf done";
    #[cfg(windows)]
    let producer = "[Console]::Out.Write('done')";
    let output =
        super::probe::run(producer, dir.path(), super::probe::PROBE_RUNTIME, &owner).unwrap();
    let mut called = 0;
    super::scheduler::advance(
        &store,
        &lease,
        &output,
        || {
            super::provenance::Classification::regex(super::machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |job, transition, handler, _| {
            assert_eq!(super::routes::action_name(&handler.action), "command");
            let current = store.lock();
            let accepted = current
                .get(&job.id)
                .unwrap()
                .transitions
                .iter()
                .find(|accepted| accepted.id == transition)
                .unwrap();
            assert!(accepted
                .handlers
                .iter()
                .any(|result| result.handler_id == handler.id && result.claim_id.is_some()));
            called += 1;
            Ok(super::handlers::Status::Completed.into())
        },
        |_, _, _| Ok(()),
    );
    assert_eq!(called, 1);
    assert_eq!(
        store.lock().get(&job.id).unwrap().transitions[0].handlers[0].status,
        super::handlers::Status::Completed
    );
}

#[test]
fn sentinel_command_held_store_team_removal_wakes_without_false_durable_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(
        path.clone(),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create(
        &mut store,
        "command",
        serde_json::to_value(parameters()).unwrap(),
    )
    .unwrap();
    let lease = store.due().unwrap().remove(0);
    let wake = Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));
    let _registration = lease.cancelled.register(wake.clone()).unwrap();
    let durable_before = std::fs::read(&path).unwrap();
    crate::persisted_state::StateFile::new(&path).hold_for_repair();
    assert!(store.remove_team("team").is_err());
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(store.get(&job.id).is_none());
    assert_eq!(std::fs::read(&path).unwrap(), durable_before);
}
