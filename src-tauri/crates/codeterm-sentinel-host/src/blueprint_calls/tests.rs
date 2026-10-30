use super::*;
#[test]
fn blueprint_v2_call_context_retains_exact_parent_identity() {
    let mut context = CallContext::default();
    let pin = CallPin {
        id: "mem:callee".into(),
        hash: "a".repeat(64),
    };
    context
        .enter("call-node", &pin, JournalMarker::Step(4))
        .unwrap();
    let parent = context.parent().unwrap();
    assert_eq!(parent.marker, JournalMarker::Step(4));
    assert_eq!(parent.frame.id, "mem:callee");
    assert_eq!(parent.frame.hash, "a".repeat(64));
    assert_eq!(
        context.enter("recursive", &pin, JournalMarker::Step(5)),
        Err(JournalReason::ContractViolation)
    );
    context.exit();
    assert!(context.parent().is_none());
}

use crate::pipeline::StepExecution;
use crate::store::{Lease, Owner, Store};
use crate::SentinelExt;
use codeterm_blueprint_types::blueprint_document::{self, Port};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::persistence_test_support::{close, ServiceInstance};
use std::sync::Arc;

fn port() -> Value {
    json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"],"additionalProperties":false})
}
fn source(fail: bool) -> Document {
    let command: String = if fail {
        "exit 9".into()
    } else {
        if cfg!(windows) {
            "$p=[Console]::In.ReadToEnd() | ConvertFrom-Json; @{n=($p.value.n+1)} | ConvertTo-Json -Compress".into()
        } else {
            r#"IFS= read -r input; n=${input#*\"n\":}; n=${n%%\}*}; printf '{"n":%s}\n' "$((n+1))""#
                .into()
        }
    };
    let value = json!({"schema_version":2,"kind":"pipeline","name":"real-call","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"invoke","call":{"ref":"increment","with":{"p":{"lit":{"n":7}}}},"out":port()}],"blueprints":{"increment":{"inputs":{"p":port()},"steps":[{"id":"increment","run":{"lit":command},"in":port(),"out":port()},{"id":"fixed-sink","uses":"notify","in":port()}],"result":{"type":port(),"from":"increment.out"}}},"result":{"to":"notify"}});
    let source = serde_yaml::to_string(&value).unwrap();
    let closure = calls::resolve_local("fixture.yaml", &source).unwrap();
    assert!(calls::check_facts(&closure).1.is_empty());
    closure.document()
}
struct Receipt {
    output: Result<ExecutionEnvelope, JournalReason>,
    notices: Vec<String>,
    journal: Vec<codeterm_storage::BlueprintJournalEntry>,
    effects: Vec<SimulatedEffect>,
    child_pids: Vec<u32>,
    stop: Option<CallStop>,
    view: Value,
}
#[derive(Clone, Copy)]
enum SavedCase {
    Keep,
    Drift,
    MissingHash,
    MissingCallee,
}
fn run(document: Document, simulate: bool) -> Receipt {
    run_saved(document, simulate, None)
}
fn run_saved(document: Document, simulate: bool, saved_case: Option<SavedCase>) -> Receipt {
    run_fixture(document, simulate, saved_case, false)
}
fn run_fixture(
    mut document: Document,
    simulate: bool,
    saved_case: Option<SavedCase>,
    restored_sink: bool,
) -> Receipt {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, creator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&creator)
        .unwrap()
        .team_id;
    mux.get_tab_mut(&creator).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    if saved_case.is_some() {
        mux.get_tab_mut(&creator).unwrap().role = Some(codeterm_types::agent::AgentRole::Manager);
    }
    if restored_sink {
        let tab = mux.get_tab_mut(&creator).unwrap();
        tab.agent_type = Some("restored-call-recipient".into());
        tab.agent_session_id = Some("call-sink-session".into());
        tab.session_binding_nonce = Some("saved-call-sink-binding".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    }
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let service = &instance.fixture.service;
    let clock = codeterm_service_core::cron::clock::MockClock::starting_at(1000);
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(&instance.storage, clock),
        service.ui.clone(),
    );
    crate::manage_fixture_ports(service, &handle);
    if let Some(case) = saved_case {
        use crate::BlueprintMemoryExt;
        let caller = service.sentinel_caller(&creator, None).unwrap();
        let original = document
            .dependencies
            .values()
            .next()
            .unwrap()
            .document
            .as_ref()
            .clone();
        for tags in [
            vec![],
            vec!["githb".into()],
            vec!["ci".into(), "git".into(), "http".into(), "health".into()],
        ] {
            assert!(service
                .blueprint_keep_callable(
                    &caller,
                    original.clone(),
                    &[],
                    &["Increment typed counter".into()],
                    &tags
                )
                .unwrap_err()
                .contains("blueprint_tags_invalid"));
        }
        let kept = service
            .blueprint_keep_callable(
                &caller,
                original.clone(),
                &[],
                &["Increment typed counter".into()],
                &["data-transform".into()],
            )
            .unwrap();
        let id = kept["id"].as_str().unwrap();
        let raw = id.strip_prefix("mem:").unwrap();
        assert_eq!(kept["tags"], json!(["data-transform"]));
        assert_eq!(
            show_callable(service, &caller, id).unwrap()["tags"],
            json!(["data-transform"])
        );
        assert_eq!(handle.store.lock().jobs.len(), 0, "keep schedules no job");
        assert!(service.notif.lock().list(false).is_empty());
        let recall = service
            .blueprint_recall(&caller, "Increment typed counter")
            .unwrap();
        assert!(
            recall
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == raw),
            "{recall}"
        );
        document.blueprints.clear();
        document.imports.clear();
        document.dependencies.clear();
        document.steps[0].call.as_mut().unwrap().reference = id.into();
        document.steps[0].call.as_mut().unwrap().pin = None;
        document = pin_bundle(service, &caller, document, &[]).unwrap();
        let pinned = document.steps[0]
            .call
            .as_ref()
            .unwrap()
            .pin
            .as_ref()
            .unwrap()
            .clone();
        assert_eq!(pinned.hash, kept["hash"]);
        assert_eq!(
            show_callable(service, &caller, id).unwrap()["hash"],
            kept["hash"]
        );
        match case {
            SavedCase::Keep => {}
            SavedCase::Drift => {
                let mut head = original.clone();
                head.note = Some("Changed head after pin".into());
                head.steps[0].run = Some(ValueForm::literal("echo 99"));
                service
                    .mem
                    .update(
                        raw,
                        codeterm_service_core::mem::store::MemUpdate {
                            body: Some(head.yaml().unwrap()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let drift = service
                    .mem
                    .blueprint_call_drift(
                        &format!("blueprints:{workspace}"),
                        &team,
                        document.clone(),
                    )
                    .unwrap();
                assert_eq!(drift.len(), 1);
                assert!(drift[0].diff.changed["increment"].contains(&"run".into()));
                service.mem.dream_blueprints().unwrap();
            }
            SavedCase::MissingHash => {
                let mut snapshot = document.dependencies.remove(&pinned.key()).unwrap();
                snapshot.document.note = Some("Unretained snapshot".into());
                snapshot.hash =
                    blueprint_document::projection::identity(&snapshot.document).unwrap();
                document.steps[0].call.as_mut().unwrap().pin = Some(snapshot.pin());
                document.dependencies.insert(snapshot.pin().key(), snapshot);
            }
            SavedCase::MissingCallee => {
                service.mem.delete(raw).unwrap();
            }
        }
    }
    let (instance, handle) = if matches!(saved_case, Some(SavedCase::Drift)) {
        drop(handle);
        let restarted = instance.restart(root.path());
        let service = &restarted.fixture.service;
        let handle = crate::scheduler::fixture_handle_with_ui(
            Store::with_storage(
                &restarted.storage,
                codeterm_service_core::cron::clock::MockClock::starting_at(1000),
            ),
            service.ui.clone(),
        );
        crate::manage_fixture_ports(service, &handle);
        (restarted, handle)
    } else {
        (instance, handle)
    };
    let service = &instance.fixture.service;
    if matches!(saved_case, Some(SavedCase::Drift)) {
        assert!(
            service
                .mux
                .read()
                .get_tab(&creator)
                .unwrap()
                .restored_from_snapshot
        );
        codeterm_service_core::service::mark_tab_agent_launch_started(&service.mux, &creator);
    }
    let deliveries = if restored_sink {
        instance.fixture.record_agent_deliveries(&creator)
    } else {
        instance.fixture.bind_recorded_agent_recipient(
            &creator,
            "call-sink-session",
            "call-sink-binding",
        )
    };
    {
        let mux = service.mux.read();
        let recipient = mux.get_tab(&creator).unwrap();
        assert_eq!(recipient.restored_from_snapshot, restored_sink);
        assert!(recipient.launch_session_authenticated());
        if !restored_sink {
            assert_eq!(
                recipient.pty_id.as_deref(),
                Some(format!("recorded-{creator}").as_str())
            );
        }
    }
    let sink_pad = service.ensure_agent_pad(&creator).unwrap();
    let sink_inbox = sink_pad.path().join("inbox");
    assert!(
        !sink_inbox.exists(),
        "keep and pin must not deliver an effect"
    );
    let owner = Owner {
        creator_tab_id: creator,
        team_id: Some(team),
        workspace_id: workspace,
        cwd: root.path().to_string_lossy().into_owned(),
    };
    // The fixture admits the static graph separately; depth cases exercise the runtime guard.
    let static_check = FrozenClosure::from_document(document.clone());
    let runtime = if static_check.is_err() {
        assert!(
            simulate,
            "invalid depth fixtures never enter durable storage"
        );
        let mut projection = document.clone();
        projection.dependencies.clear();
        projection.blueprints.clear();
        projection.imports.clear();
        for node in &mut projection.steps {
            node.call = None;
            node.uses = Some(blueprint_document::NodeKind::Filter);
            node.params = BTreeMap::from([("pattern".into(), ValueForm::literal(".*"))]);
            node.output = None;
        }
        projection.lower().unwrap()
    } else {
        document.lower().unwrap()
    };
    let spec = serde_json::from_value(runtime["spec"].clone()).unwrap();
    let mut job = handle
        .store
        .lock()
        .prepare_pipeline(spec, owner, false)
        .unwrap();
    if static_check.is_ok() {
        job.document = Some(json!(document));
    } else {
        assert!(static_check
            .unwrap_err()
            .contains("budget_exhausted: call depth"));
    }
    let mut job = if simulate {
        job
    } else {
        crate::storage::commit(&handle.store, |store| store.stage_insert(job)).unwrap()
    };
    job.document = Some(json!(document));
    let step = calls::lower::node(&document, &document.steps[0]).unwrap();
    let lease = Lease {
        job: job.clone(),
        cancelled: Arc::new(crate::probe::cancellation::Cancellation::new(false)),
        event: None,
    };
    let spawned = lease.cancelled.observe_spawned();
    let mut ctx = RunCtx::new(&handle, service, &lease, "call-proof");
    ctx.call_state.simulate = simulate;
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let started = std::time::Instant::now();
    let result = execute_step(&mut ctx, &step, ExecutionEnvelope::from_text(""));
    if restored_sink {
        assert!(
            result.is_ok(),
            "a restored recipient must admit the call result durably: {:?}",
            result.as_ref().err()
        );
        deliveries.wait_for_flush();
        assert!(
            service
                .mux
                .read()
                .get_tab(&job.creator_tab_id)
                .unwrap()
                .restored_from_snapshot
        );
        assert!(instance
            .fixture
            .has_pending_agent_delivery(&job.creator_tab_id));
        assert!(deliveries.texts.lock().is_empty());
        let files = std::fs::read_dir(&sink_inbox)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            files.len(),
            1,
            "the original durable result is retained while held"
        );
        let body = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(body.ends_with("{\"n\":8}"));
        instance.fixture.resume_recorded_agent_recipient(
            &job.creator_tab_id,
            "call-sink-session",
            "first-call-sink-binding",
        );
        deliveries.wait_for_flush();
        assert!(!instance
            .fixture
            .has_pending_agent_delivery(&job.creator_tab_id));
        assert_eq!(deliveries.texts.lock().len(), 1);
        assert!(deliveries.texts.lock()[0].starts_with("Original run: call-proof; timestamp: "));
        instance.fixture.resume_recorded_agent_recipient(
            &job.creator_tab_id,
            "call-sink-session",
            "second-call-sink-binding",
        );
        deliveries.wait_for_flush();
        assert_eq!(
            deliveries.texts.lock().len(),
            1,
            "a second rebind cannot replay the result"
        );
    }
    let child_pids = spawned.try_iter().collect::<Vec<_>>();
    if matches!(result, Err(JournalReason::Deadline)) {
        use std::io::Write;
        writeln!(std::io::stdout().lock(), "call fixture Deadline: test={:?} test_binary_pid={} child_pids={child_pids:?} started_unix_ms={started_at} ended_unix_ms={} elapsed_ms={} remaining_ms={} native_command_cap_ms={} stop={:?}", std::thread::current().name(), std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), started.elapsed().as_millis(), ctx.remaining().as_millis(), crate::probe::PROBE_RUNTIME.as_millis(), ctx.call_state.stop).unwrap();
    }
    let output = result.map(|step: StepExecution| step.output);
    let effects = ctx.call_state.effects.clone();
    let stop = ctx.call_state.stop.clone();
    drop(ctx);
    if simulate {
        assert!(
            child_pids.is_empty(),
            "simulation must not launch a process"
        );
        assert!(
            handle.store.lock().jobs.is_empty(),
            "simulation must not schedule a job"
        );
    }
    let journal = instance
        .storage
        .sentinels()
        .blueprint_log(
            codeterm_storage::SentinelId::new(job.id.clone()).unwrap(),
            200,
        )
        .wait(crate::storage::COMMIT_DEADLINE)
        .unwrap()
        .value;
    let delivered = deliveries.texts.lock().clone();
    let bodies = if sink_inbox.exists() {
        std::fs::read_dir(&sink_inbox)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let pointer = format!("Read the result in {}", path.display());
                assert_eq!(
                    delivered
                        .iter()
                        .filter(|text| text.contains(&pointer))
                        .count(),
                    1,
                    "each durable result must be delivered exactly once"
                );
                std::fs::read_to_string(path).unwrap()
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    assert_eq!(delivered.len(), bodies.len(), "no pointer without a body");
    if !restored_sink {
        assert!(
            delivered
                .iter()
                .all(|body| !body.starts_with("Original run:")),
            "live delivery keeps the existing pointer body"
        );
        assert!(
            bodies.iter().all(|body| !body.starts_with("Original run:")),
            "live durable files keep their existing body"
        );
    }
    let notices = bodies
        .iter()
        .map(|body| {
            let (_, payload) = body.split_once("Probe output (untrusted data):\n").unwrap();
            serde_json::from_str::<Value>(payload).unwrap().to_string()
        })
        .collect();
    let view = project(&document, &journal, &Default::default()).unwrap_or(Value::Null);
    drop(handle);
    drop(lease);
    drop(deliveries);
    close(instance);
    Receipt {
        output,
        notices,
        journal,
        effects,
        child_pids,
        stop,
        view,
    }
}

fn pure_source() -> Document {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.steps[0].run = None;
    callee.document.steps[0].uses = Some(blueprint_document::NodeKind::Filter);
    callee.document.steps[0]
        .params
        .insert("pattern".into(), ValueForm::literal(".*"));
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    let call = document.steps[0].call.as_mut().unwrap();
    call.arguments
        .insert("p".into(), ValueForm::literal(json!({"n":8})));
    call.pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    document
}

#[test]
fn blueprint_v2_call_restored_result_stays_pending_then_delivers_once_after_rebind() {
    let receipt = run_fixture(pure_source(), false, None, true);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, ["{\"n\":8}"]);
    assert!(receipt.child_pids.is_empty());
    assert!(receipt
        .journal
        .iter()
        .any(|row| row.step_name == "fixed-sink" && row.state == JournalState::Done));
}
#[test]
fn blueprint_v2_call_distinct_result_effects_in_one_run_are_both_delivered_once() {
    let mut document = pure_source();
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    let mut second = callee.document.steps[1].clone();
    second.id = "second-fixed-sink".into();
    callee.document.steps.push(second);
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, false);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices.len(), 2);
    assert!(receipt.notices.iter().all(|payload| payload == "{\"n\":8}"));
    for id in ["fixed-sink", "second-fixed-sink"] {
        assert_eq!(
            receipt
                .journal
                .iter()
                .filter(|row| row.step_name == id && row.state == JournalState::Done)
                .count(),
            1
        );
    }
}

#[test]
fn blueprint_v2_call_real_executor_delivers_eight_to_actual_local_notification_sink() {
    let receipt = run(source(false), false);
    assert_eq!(receipt.output.as_ref().unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, vec![json!({"n":8}).to_string()]);
    assert!(receipt.effects.is_empty());
    assert_eq!(
        receipt.child_pids.len(),
        1,
        "native child adoption must be observable"
    );
    assert!(receipt.child_pids[0] > 1);
    let sink = receipt
        .journal
        .iter()
        .find(|row| row.step_name == "fixed-sink" && row.state == JournalState::Done)
        .unwrap();
    assert_eq!(sink.call_stack.len(), 1);
    assert_eq!(sink.call_stack[0].node, "invoke");
    assert!(sink.call_stack[0].id.contains("increment"));
    assert_eq!(sink.call_stack[0].hash.len(), 64);
    assert_eq!(
        receipt.view["expanded_calls"][0]["graph"]["steps"][0]["id"],
        "increment"
    );
    assert!(receipt.view["nested_run"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["step_name"] == "fixed-sink"
            && n["call_stack"][0]["hash"] == sink.call_stack[0].hash));
}
#[test]
fn blueprint_v2_call_simulate_records_nested_effect_provenance_and_delivers_zero() {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.steps[0].run = None;
    callee.document.steps[0].uses = Some(blueprint_document::NodeKind::Filter);
    callee.document.steps[0]
        .params
        .insert("pattern".into(), ValueForm::literal(".*"));
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    let call = document.steps[0].call.as_mut().unwrap();
    call.arguments
        .insert("p".into(), ValueForm::literal(json!({"n":8})));
    call.pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, true);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert!(receipt.notices.is_empty());
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].effect["payload"], json!({"n":8}));
    assert_eq!(receipt.effects[0].call_stack[0].node, "invoke");
    assert_eq!(receipt.effects[0].scope, "simulation");
    assert!(receipt.journal.is_empty());
    assert!(
        receipt.child_pids.is_empty(),
        "simulation must launch no process"
    );
}
#[test]
fn blueprint_v2_call_nested_failure_stops_before_later_effect_and_retains_stack() {
    let receipt = run(source(true), false);
    assert!(receipt.output.is_err());
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "fixed-sink"));
    let stop = receipt.stop.unwrap();
    assert_eq!(stop.node, "increment");
    assert_eq!(stop.call_stack[0].node, "invoke");
    assert!(receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"
            && row.state == JournalState::Failed
            && !row.call_stack.is_empty()));
}
fn chain(depth: usize) -> Document {
    let object: Port = serde_json::from_value(port()).unwrap();
    let mut dependencies = BTreeMap::new();
    let mut next: Option<CallPin> = None;
    for index in (0..depth).rev() {
        let mut body:Document=serde_json::from_value(json!({"schema_version":2,"kind":"pipeline","name":format!("callee-{index}"),"inputs":{"p":port()},"steps":[{"id":"leaf","uses":"filter","with":{"pattern":{"lit":".*"}},"in":port(),"out":port()}],"result":{"type":port(),"from":"leaf.out"}})).unwrap();
        if let Some(pin) = next {
            body.steps[0].uses = None;
            body.steps[0].params.clear();
            body.steps[0].call = Some(Call {
                reference: pin.id.clone(),
                pin: Some(pin),
                arguments: BTreeMap::from([("p".into(), ValueForm::input("p"))]),
            });
        }
        let hash = blueprint_document::projection::identity(&body).unwrap();
        let callee = calls::FrozenCallee {
            id: format!("inline:callee-{index}"),
            hash,
            document: Box::new(body),
            locations: BTreeMap::new(),
            source_hash: None,
        };
        next = Some(callee.pin());
        dependencies.insert(callee.pin().key(), callee);
    }
    let mut root = source(false);
    root.dependencies = dependencies;
    root.blueprints.clear();
    root.steps[0].call.as_mut().unwrap().pin = next;
    root.steps[0].output = Some(object);
    root
}
#[test]
fn blueprint_v2_call_simulate_depth_eight_succeeds_depth_nine_budget_exhausted() {
    assert!(FrozenClosure::from_document(chain(8)).is_ok());
    assert!(FrozenClosure::from_document(chain(9))
        .unwrap_err()
        .contains("budget_exhausted"));
    let eight = run(chain(8), true);
    assert_eq!(eight.output.unwrap().value, json!({"n":7}));
    assert!(eight.notices.is_empty());
    let nine = run(chain(9), true);
    assert_eq!(nine.output.unwrap_err(), JournalReason::BudgetExhausted);
    assert!(nine.notices.is_empty());
}
#[test]
fn blueprint_v2_call_callee_grant_widening_refuses_before_any_nested_claim() {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.steps[0]
        .caps
        .push("net:forbidden.test".into());
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, false);
    assert_eq!(
        receipt.output.unwrap_err(),
        JournalReason::ContractViolation
    );
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"));
}
#[test]
fn blueprint_v2_call_out_of_scope_effect_target_refuses_before_nested_claim() {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.steps[1].uses = Some(blueprint_document::NodeKind::Send);
    callee.document.steps[1]
        .params
        .insert("target".into(), ValueForm::literal("foreign-workspace-tab"));
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, false);
    assert_eq!(
        receipt.output.unwrap_err(),
        JournalReason::ContractViolation
    );
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"));
}

#[test]
fn blueprint_v2_call_keep_recall_pin_and_real_execute_delivers_exact_saved_eight() {
    let receipt = run_saved(source(false), false, Some(SavedCase::Keep));
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, vec![json!({"n":8}).to_string()]);
    assert!(receipt.journal.iter().any(|row| row
        .call_stack
        .iter()
        .any(|frame| frame.id.starts_with("mem:") && frame.hash.len() == 64)));
}
#[test]
fn blueprint_v2_call_head_edit_and_dream_warn_while_real_execution_uses_pinned_body() {
    let receipt = run_saved(source(false), false, Some(SavedCase::Drift));
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, vec![json!({"n":8}).to_string()]);
}
#[test]
fn blueprint_v2_call_missing_mem_hash_or_callee_refuses_before_any_nested_effect() {
    for case in [SavedCase::MissingHash, SavedCase::MissingCallee] {
        let receipt = run_saved(source(false), false, Some(case));
        assert!(receipt.output.is_err());
        assert!(receipt.notices.is_empty());
        assert!(!receipt
            .journal
            .iter()
            .any(|row| row.step_name == "increment" || row.step_name == "fixed-sink"));
    }
}

#[test]
fn blueprint_v2_call_scheduler_owned_effects_refuse_at_runtime_before_any_nested_effect() {
    for kind in ["blueprint", "stop", "respond_prompt"] {
        let mut document = source(false);
        let key = document.dependencies.keys().next().unwrap().clone();
        let mut frozen = document.dependencies.remove(&key).unwrap();
        frozen.document.steps[1].uses = Some(serde_json::from_value(json!(kind)).unwrap());
        frozen.hash = blueprint_document::projection::identity(&frozen.document).unwrap();
        document.steps[0].call.as_mut().unwrap().pin = Some(frozen.pin());
        document.dependencies.insert(frozen.pin().key(), frozen);
        let receipt = run(document, false);
        assert_eq!(
            receipt.output.unwrap_err(),
            JournalReason::RuntimeUnavailable
        );
        assert!(receipt.notices.is_empty());
        assert!(!receipt
            .journal
            .iter()
            .any(|row| row.step_name == "increment" || row.step_name == "fixed-sink"));
    }
}

#[test]
fn blueprint_v2_call_default_arguments_feed_the_typed_callee_input() {
    let mut document = source(false);
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    callee.document.inputs.get_mut("p").unwrap().default = Some(json!({"n":7}));
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    let call = document.steps[0].call.as_mut().unwrap();
    call.arguments.clear();
    call.pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
    let receipt = run(document, false);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, vec![json!({"n":8}).to_string()]);
}

#[path = "review.rs"]
mod review;
