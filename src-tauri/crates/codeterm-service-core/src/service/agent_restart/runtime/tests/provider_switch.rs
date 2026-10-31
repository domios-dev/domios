use super::*;
use crate::multiplexer::types::TaskBinding;
use crate::plan::{OrchestratorRef, Plan, PlanMode};
use crate::service::plan::{task_assignment::AssignmentRuntimeBinding, NewTask};

fn complete_switch_task(fixture: &RuntimeFixture, task_id: &str) {
    let detail = fixture.service.plan_store.show_task(task_id).unwrap();
    fixture
        .service
        .plan_store
        .with_plan(&detail.plan_id, 5, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = crate::plan::TaskStatus::Done;
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.execution_attempts[0].closed_at = Some(5);
            Ok(())
        })
        .unwrap();
}

#[test]
fn completed_worker_switch_preserves_history_without_task_authority_and_delivers_once() {
    use crate::plan::AgentRef;
    use crate::service::plan::{
        interruption::TaskMutationAuthority, task_attempts::BoundWorkerReport,
    };
    let (fixture, task_id, attempt_id) = switch_worker_fixture();
    let live_source = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    let live_input = fixture
        .service
        .provider_switch_input(&live_source, &[], std::path::Path::new("archive-token"))
        .unwrap();
    let live_frame = codeterm_wire::message_markers::parse_host_injected(&live_input).unwrap();
    complete_switch_task(&fixture, &task_id);
    let store = &fixture.service.plan_store;
    let before = store.show_task(&task_id).unwrap();
    let source = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    let pad = fixture
        .service
        .ensure_agent_pad(&fixture.tab)
        .unwrap()
        .path()
        .to_path_buf();
    let outcome = fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            true,
        )
        .unwrap();
    assert!(outcome.binding_restored && outcome.handoff_delivered);
    let target = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    assert_eq!(target.id, source.id);
    assert_eq!(target.team_id, source.team_id);
    assert_eq!(target.parent_tab_id, source.parent_tab_id);
    assert_eq!(target.cwd, source.cwd);
    assert_eq!(target.task_binding, source.task_binding);
    assert_eq!(
        fixture
            .service
            .ensure_agent_pad(&fixture.tab)
            .unwrap()
            .path(),
        pad
    );
    assert_ne!(target.agent_session_id, source.agent_session_id);
    assert_eq!(store.show_task(&task_id).unwrap().task, before.task);
    assert!(!fixture
        .service
        .restart_carries_worker_interruption(&fixture.tab));
    assert!(fixture
        .service
        .carry_authorized_runtime_restart_binding(
            &fixture.tab,
            &outcome.session_id,
            target.session_binding_nonce.as_deref().unwrap(),
        )
        .unwrap());
    assert!(store
        .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
            &fixture.tab,
            Some(&before.plan_id),
            Some(&task_id),
            Some((
                &outcome.session_id,
                target.session_binding_nonce.as_deref().unwrap()
            )),
            6,
        )
        .is_err());
    let report = store.ingest_bound_worker_report(
        &before.plan_id,
        &task_id,
        &AgentRef::from_tab(&fixture.tab),
        BoundWorkerReport {
            message: "forbidden terminal report".into(),
            status: Some(codeterm_types::agent::ReportStatus::InReview),
            authority: Some(TaskMutationAuthority {
                tab_id: fixture.tab.clone(),
                session_id: outcome.session_id.clone(),
                session_binding_nonce: target.session_binding_nonce.clone().unwrap(),
                owner_epoch: before.task.interruption.owner_epoch,
            }),
            ..Default::default()
        },
        7,
    );
    assert!(matches!(report, Ok(false)));
    assert_eq!(store.show_task(&task_id).unwrap().task, before.task);
    let requests = fixture.requests();
    let submissions = requests
        .iter()
        .filter(|frame| frame["method"] == "submitTurn")
        .collect::<Vec<_>>();
    assert_eq!(submissions.len(), 1);
    let payload = serde_json::to_string(&submissions[0]["params"]).unwrap();
    assert_eq!(payload.matches("provider-handoff").count(), 1);
    assert!(payload.contains(&attempt_id));
    let input = fixture
        .service
        .provider_switch_input(&source, &[], std::path::Path::new("archive-token"))
        .unwrap();
    let frame = codeterm_wire::message_markers::parse_host_injected(&input).unwrap();
    assert_ne!(
        frame.payload.split("\n\n").next(),
        live_frame.payload.split("\n\n").next()
    );
    let context: serde_json::Value =
        serde_json::from_str(frame.payload.split("\n\n").nth(1).unwrap()).unwrap();
    assert_eq!(context["task_context"]["task"]["status"], "done");
}

fn register_completed_switch_runtime(fixture: &RuntimeFixture, provider: &str) {
    let methods = [
        "initialize",
        "openThread",
        "resumeThread",
        "readHistory",
        "submitTurn",
        "control",
    ]
    .into_iter()
    .map(|kind| (kind.to_string(), json!([kind])))
    .collect::<serde_json::Map<String, Value>>();
    let manifest = PluginManifest::from_json_str(&json!({
        "id":provider,"version":"1.0.0","displayName":"Next fixture","binaryName":"node",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtimeModes":[{"id":"auto-accept-edits","displayName":"Auto accept edits"}],
                "runtime":"stdio-jsonrpc","command":{"binary":"node","args":[]},
                "requestMethods":methods,"notificationMethods":[]}},
        "spawn":{"readinessStrategy":{"kind":"poll_shell_prompt","timeoutSecs":1},
            "taskDelivery":"inline_flag","systemPromptDelivery":{"kind":"cli_flag","flag":"--opaque-system"},
            "verifySubmit":{"kind":"none"}},
        "permissions":{"subprocess":{"allow":["node"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(provider, SOURCE.into()).unwrap()),
        )));
}

fn clear_switch_cooldown(fixture: &RuntimeFixture) {
    fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .remove(&fixture.tab);
}

fn assert_completed_session_has_no_task_authority(fixture: &RuntimeFixture, task_id: &str) {
    use crate::service::plan::{
        interruption::TaskMutationAuthority, task_attempts::BoundWorkerReport,
    };
    let detail = fixture.service.plan_store.show_task(task_id).unwrap();
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    let thread = current.agent_session_id.unwrap();
    let nonce = current.session_binding_nonce.unwrap();
    assert!(fixture
        .service
        .plan_store
        .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
            &fixture.tab,
            Some(&detail.plan_id),
            Some(task_id),
            Some((&thread, &nonce)),
            8,
        )
        .is_err());
    let report = fixture
        .service
        .plan_store
        .ingest_bound_worker_report(
            &detail.plan_id,
            task_id,
            &crate::plan::AgentRef::from_tab(&fixture.tab),
            BoundWorkerReport {
                message: "forbidden completed-session report".into(),
                status: Some(codeterm_types::agent::ReportStatus::InReview),
                authority: Some(TaskMutationAuthority {
                    tab_id: fixture.tab.clone(),
                    session_id: thread,
                    session_binding_nonce: nonce,
                    owner_epoch: detail.task.interruption.owner_epoch,
                }),
                ..Default::default()
            },
            9,
        )
        .unwrap();
    assert!(!report);
    assert!(!fixture.service.signal_worker_interruption(
        &fixture.tab,
        crate::service::worker_interruption::WorkerInterruptionEvent::ProcessExited,
    ));
    assert_eq!(
        fixture.service.plan_store.show_task(task_id).unwrap().task,
        detail.task
    );
}

#[test]
fn completed_worker_successive_switches_and_resume_keep_history_and_exact_session_continuity() {
    let (fixture, task_id, _) = switch_worker_fixture();
    register_completed_switch_runtime(&fixture, "next-runtime");
    complete_switch_task(&fixture, &task_id);
    let original = fixture.service.plan_store.show_task(&task_id).unwrap();
    let strides = fixture
        .service
        .plan_store
        .get(&original.plan_id)
        .unwrap()
        .strides
        .len();
    let binding = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .task_binding
        .clone();
    let mut threads = Vec::new();
    let mut nonces = Vec::new();
    for provider in [PROVIDER, "next-runtime"] {
        clear_switch_cooldown(&fixture);
        let outcome = fixture
            .service
            .switch_provider_in_place(
                &fixture.tab,
                provider,
                Some("supplied-model".into()),
                None,
                true,
            )
            .unwrap();
        assert!(outcome.binding_restored && outcome.handoff_delivered);
        threads.push(outcome.session_id);
        let current = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .cloned()
            .unwrap();
        assert_eq!(current.agent_type.as_deref(), Some(provider));
        assert_eq!(current.task_binding, binding);
        nonces.push(current.session_binding_nonce.clone().unwrap());
        assert_eq!(
            current.reported_session_binding_nonce,
            current.session_binding_nonce
        );
        assert!(agent_runtime::engine::has_live_bound_session(&current));
        assert_eq!(
            fixture.service.plan_store.show_task(&task_id).unwrap().task,
            original.task
        );
        assert!(!fixture
            .service
            .restart_carries_worker_interruption(&fixture.tab));
        assert_completed_session_has_no_task_authority(&fixture, &task_id);
        let requests = fixture.requests();
        let sends = requests
            .iter()
            .filter(|frame| frame["method"] == "submitTurn")
            .collect::<Vec<_>>();
        assert_eq!(sends.len(), threads.len());
        assert_eq!(
            sends.last().unwrap()["params"]["threadId"],
            *threads.last().unwrap()
        );
    }
    assert_ne!(threads[0], threads[1]);
    assert_ne!(nonces[0], nonces[1]);
    clear_switch_cooldown(&fixture);
    let resumed = fixture
        .service
        .agent_restart_observed(&fixture.tab, true, 0)
        .unwrap();
    assert!(resumed.agent_ready && resumed.binding_restored);
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    assert_eq!(current.agent_session_id.as_ref(), threads.last());
    assert_ne!(current.session_binding_nonce.as_ref(), nonces.last());
    assert_eq!(
        current.reported_session_binding_nonce,
        current.session_binding_nonce
    );
    assert_eq!(current.task_binding, binding);
    assert!(agent_runtime::engine::has_live_bound_session(&current));
    assert!(current.boot_failure_reason.is_none());
    assert!(fixture
        .service
        .report_runtime_worker_session_finalization(
            &fixture.tab,
            current.agent_session_id.as_deref().unwrap(),
            current.session_binding_nonce.as_deref().unwrap(),
        )
        .is_none());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .task_binding,
        binding
    );
    assert_eq!(
        fixture
            .service
            .plan_store
            .get(&original.plan_id)
            .unwrap()
            .strides
            .len(),
        strides
    );
    assert_eq!(
        fixture.service.plan_store.show_task(&task_id).unwrap().task,
        original.task
    );
    assert!(!fixture
        .service
        .restart_carries_worker_interruption(&fixture.tab));
    assert_completed_session_has_no_task_authority(&fixture, &task_id);
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|frame| frame["method"] == "submitTurn")
            .count(),
        2
    );
    let resume = requests
        .iter()
        .rev()
        .find(|frame| frame["method"] == "resumeThread")
        .unwrap();
    assert_eq!(resume["params"]["threadId"], threads[1]);
}

#[test]
fn completed_current_session_receipt_refuses_stale_identity_generation_history_and_host_loss() {
    for invalid in [
        "provider",
        "session",
        "nonce",
        "unverified",
        "generation",
        "history",
        "status",
        "binding",
        "partial",
        "host_loss",
    ] {
        let (fixture, task_id, _) = switch_worker_fixture();
        complete_switch_task(&fixture, &task_id);
        fixture
            .service
            .switch_provider_in_place(
                &fixture.tab,
                PROVIDER,
                Some("supplied-model".into()),
                None,
                true,
            )
            .unwrap();
        let original = fixture.service.plan_store.show_task(&task_id).unwrap();
        if invalid == "generation" {
            agent_runtime::engine::for_tab(&fixture.tab)
                .unwrap()
                .context
                .lock()
                .epoch += 1;
        } else if matches!(invalid, "history" | "status") {
            fixture
                .service
                .plan_store
                .with_plan(&original.plan_id, 6, |plan| {
                    let task = plan
                        .tasks
                        .iter_mut()
                        .find(|task| task.id == task_id)
                        .unwrap();
                    if invalid == "status" {
                        task.status = crate::plan::TaskStatus::InReview;
                    } else {
                        task.execution_attempts[0].reasoning_effort = Some("foreign".into());
                    }
                    Ok(())
                })
                .unwrap();
        } else if invalid == "host_loss" {
            fixture
                .service
                .forget_completed_session_for_test(&fixture.tab);
        } else {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&fixture.tab).unwrap();
            match invalid {
                "provider" => tab.agent_type = Some("foreign".into()),
                "session" => tab.agent_session_id = Some("foreign".into()),
                "nonce" => tab.session_binding_nonce = Some("foreign".into()),
                "unverified" => tab.reported_session_binding_nonce = None,
                "binding" => tab.task_binding.as_mut().unwrap().plan_id = Some("foreign".into()),
                "partial" => tab.task_binding.as_mut().unwrap().attempt_id = None,
                _ => unreachable!(),
            }
        }
        let source = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .cloned()
            .unwrap();
        let before = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        let requests = fixture.requests().len();
        let error = fixture
            .service
            .authorize_provider_switch_binding(
                &fixture.tab,
                source.agent_type.as_deref().unwrap(),
                "next-runtime",
                source.session_binding_nonce.as_deref().unwrap(),
            )
            .err()
            .unwrap();
        assert!(
            error.starts_with("provider_switch_binding_refused:"),
            "{invalid}: {error}"
        );
        assert!(
            fixture
                .service
                .authorize_runtime_restart_binding(
                    &fixture.tab,
                    source.agent_type.as_deref().unwrap(),
                    source.session_binding_nonce.as_deref().unwrap(),
                    false,
                )
                .is_err(),
            "{invalid}"
        );
        let after = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .cloned()
            .unwrap();
        assert_eq!(after.task_binding, source.task_binding, "{invalid}");
        assert_eq!(after.agent_session_id, source.agent_session_id, "{invalid}");
        assert_eq!(
            fixture.service.plan_store.show_task(&task_id).unwrap().task,
            before,
            "{invalid}"
        );
        assert_eq!(fixture.requests().len(), requests, "{invalid}");
    }
}

#[test]
fn completed_resume_target_carry_fails_closed_on_history_or_generation_races() {
    for race in ["history", "generation"] {
        let (fixture, task_id, _) = switch_worker_fixture();
        complete_switch_task(&fixture, &task_id);
        let outcome = fixture
            .service
            .switch_provider_in_place(
                &fixture.tab,
                PROVIDER,
                Some("supplied-model".into()),
                None,
                true,
            )
            .unwrap();
        let source = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .cloned()
            .unwrap();
        let binding = fixture
            .service
            .authorize_runtime_restart_binding(
                &fixture.tab,
                PROVIDER,
                source.session_binding_nonce.as_deref().unwrap(),
                false,
            )
            .unwrap();
        fixture
            .service
            .validate_completed_restart_binding(&fixture.tab, binding.as_ref())
            .unwrap();
        fixture
            .service
            .stage_runtime_restart_binding(&fixture.tab, binding);
        let nonce = fixture.service.rotate_session_binding_nonce(&fixture.tab);
        {
            let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
            let mut context = session.context.lock();
            context.binding_nonce = nonce.clone();
            context.epoch += 1;
            drop(context);
            session.state.lock().epoch += 1;
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&fixture.tab)
                .unwrap()
                .reported_session_binding_nonce = Some(nonce.clone());
        }
        if race == "history" {
            let detail = fixture.service.plan_store.show_task(&task_id).unwrap();
            fixture
                .service
                .plan_store
                .with_plan(&detail.plan_id, 6, |plan| {
                    plan.tasks
                        .iter_mut()
                        .find(|task| task.id == task_id)
                        .unwrap()
                        .interruption
                        .owner_epoch += 1;
                    Ok(())
                })
                .unwrap();
        } else {
            assert!(
                fixture
                    .service
                    .carry_authorized_runtime_restart_binding(
                        &fixture.tab,
                        &outcome.session_id,
                        &nonce,
                    )
                    .unwrap()
            );
            agent_runtime::engine::for_tab(&fixture.tab)
                .unwrap()
                .context
                .lock()
                .epoch += 1;
        }
        let changed_history = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        let failure = fixture
            .service
            .carry_authorized_runtime_restart_binding(&fixture.tab, &outcome.session_id, &nonce)
            .unwrap_err();
        assert!(failure.reason.contains(race), "{}", failure.reason);
        assert!(!fixture
            .service
            .restart_carries_worker_interruption(&fixture.tab));
        assert!(fixture
            .service
            .authorize_runtime_restart_binding(&fixture.tab, PROVIDER, &nonce, false,)
            .is_err());
        assert_eq!(
            fixture.service.plan_store.show_task(&task_id).unwrap().task,
            changed_history
        );
        assert_completed_session_has_no_task_authority(&fixture, &task_id);
        assert_eq!(
            fixture
                .requests()
                .iter()
                .filter(|frame| frame["method"] == "submitTurn")
                .count(),
            1
        );
    }
}

#[test]
fn completed_worker_switch_refuses_invalid_identity_before_teardown_or_mutation() {
    for invalid in [
        "plan",
        "task",
        "attempt",
        "partial",
        "session",
        "nonce",
        "unverified",
        "provider",
        "foreign",
        "retired",
        "unacknowledged",
        "duplicate",
        "ambiguous",
        "open",
        "owner",
        "owner_epoch",
        "owner_session",
        "closed",
        "cancelled",
        "pty_id",
        "empty_pty",
        "lifecycle_epoch",
        "sleeping",
    ] {
        let (fixture, task_id, _) = switch_worker_fixture();
        complete_switch_task(&fixture, &task_id);
        let detail = fixture.service.plan_store.show_task(&task_id).unwrap();
        match invalid {
            "plan" | "task" | "attempt" | "partial" | "session" | "nonce" | "unverified"
            | "pty_id" | "empty_pty" | "lifecycle_epoch" | "sleeping" => {
                let mut mux = fixture.service.mux.write();
                let tab = mux.get_tab_mut(&fixture.tab).unwrap();
                match invalid {
                    "plan" => tab.task_binding.as_mut().unwrap().plan_id = Some("foreign".into()),
                    "task" => tab.task_binding.as_mut().unwrap().task_id = Some("foreign".into()),
                    "attempt" => {
                        tab.task_binding.as_mut().unwrap().attempt_id = Some("foreign".into())
                    }
                    "partial" => tab.task_binding.as_mut().unwrap().attempt_id = None,
                    "session" => tab.agent_session_id = Some("foreign".into()),
                    "nonce" => tab.session_binding_nonce = Some("foreign".into()),
                    "unverified" => tab.reported_session_binding_nonce = None,
                    "pty_id" => tab.pty_id = None,
                    "empty_pty" => tab.pty_id = Some(String::new()),
                    "lifecycle_epoch" => tab.lifecycle_epoch = None,
                    "sleeping" => {
                        tab.frozen = true;
                        tab.pty_id = None;
                    }
                    _ => unreachable!(),
                }
            }
            _ => fixture
                .service
                .plan_store
                .with_plan(&detail.plan_id, 6, |plan| {
                    let task = plan
                        .tasks
                        .iter_mut()
                        .find(|entry| entry.id == task_id)
                        .unwrap();
                    match invalid {
                        "provider" => task.execution_attempts[0].provider = Some("foreign".into()),
                        "foreign" => task.execution_attempts[0].executor_tab_id = "foreign".into(),
                        "retired" => task.execution_attempts[0].retired_at = Some(6),
                        "unacknowledged" => task.execution_attempts[0].acknowledged_at = None,
                        "duplicate" => task
                            .execution_attempts
                            .push(task.execution_attempts[0].clone()),
                        "ambiguous" => {
                            let mut duplicate = task.execution_attempts[0].clone();
                            duplicate.id = "other-attempt".into();
                            task.execution_attempts.push(duplicate);
                        }
                        "open" => task.execution_attempts[0].closed_at = None,
                        "owner" => task.assigned_tab_id = Some("foreign".into()),
                        "owner_epoch" => task.interruption.owner_epoch += 1,
                        "owner_session" => {
                            task.interruption.owner_session_id = Some("foreign".into())
                        }
                        "closed" => task.status = crate::plan::TaskStatus::InReview,
                        "cancelled" => task.status = crate::plan::TaskStatus::Cancelled,
                        _ => unreachable!(),
                    }
                    Ok(())
                })
                .unwrap(),
        }
        let source = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .cloned()
            .unwrap();
        let before = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        let requests = fixture.requests().len();
        let error = fixture
            .service
            .switch_provider_in_place(
                &fixture.tab,
                PROVIDER,
                Some("supplied-model".into()),
                None,
                true,
            )
            .err()
            .unwrap();
        assert!(
            error.starts_with("provider_switch_binding_refused:"),
            "{invalid}: {error}"
        );
        if matches!(
            invalid,
            "pty_id" | "empty_pty" | "lifecycle_epoch" | "sleeping"
        ) {
            assert!(error.contains("completed_continuity_pty_generation_missing:"));
            assert!(error.contains("wake or resume this tab"));
        }
        let after = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .cloned()
            .unwrap();
        assert_eq!(after.agent_type, source.agent_type, "{invalid}");
        assert_eq!(after.agent_session_id, source.agent_session_id, "{invalid}");
        assert_eq!(after.task_binding, source.task_binding, "{invalid}");
        assert_eq!(
            after.session_binding_nonce, source.session_binding_nonce,
            "{invalid}"
        );
        assert_eq!(
            after.reported_session_binding_nonce, source.reported_session_binding_nonce,
            "{invalid}"
        );
        assert_eq!(
            fixture.service.plan_store.show_task(&task_id).unwrap().task,
            before,
            "{invalid}"
        );
        assert_eq!(fixture.requests().len(), requests, "{invalid}");
    }
}

#[test]
fn completed_switch_revalidates_history_before_source_retirement() {
    let (fixture, task_id, _) = switch_worker_fixture();
    complete_switch_task(&fixture, &task_id);
    let authority = fixture
        .service
        .authorize_provider_switch_binding(
            &fixture.tab,
            "source-provider",
            PROVIDER,
            "source-nonce",
        )
        .unwrap()
        .unwrap();
    fixture
        .service
        .validate_provider_switch_binding(&fixture.tab, Some(&authority))
        .unwrap();
    let detail = fixture.service.plan_store.show_task(&task_id).unwrap();
    fixture
        .service
        .plan_store
        .with_plan(&detail.plan_id, 6, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|entry| entry.id == task_id)
                .unwrap();
            task.interruption.owner_epoch += 1;
            Ok(())
        })
        .unwrap();
    let before = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert!(fixture
        .service
        .validate_provider_switch_binding(&fixture.tab, Some(&authority))
        .is_err());
    assert_eq!(
        fixture.service.plan_store.show_task(&task_id).unwrap().task,
        before
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("source-thread"),
    );
}

#[test]
fn task_worker_switch_from_pty_carries_attempt_and_delivers_one_handoff() {
    let (fixture, task, attempt) = switch_worker_fixture();
    let store = &fixture.service.plan_store;
    assert!(fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            false
        )
        .err()
        .unwrap()
        .starts_with("provider_switch_busy:"));
    let outcome = fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            true,
        )
        .unwrap();
    assert!(outcome.binding_restored && outcome.handoff_delivered);
    assert!(fixture
        .service
        .provider_switch_history(&fixture.tab, &outcome.previous_history_path)
        .unwrap()
        .is_empty());
    let persisted = store.show_task(&task).unwrap().task;
    assert_eq!(persisted.execution_attempts.len(), 1);
    let carried = &persisted.execution_attempts[0];
    assert_eq!(carried.id, attempt);
    assert_eq!(carried.provider.as_deref(), Some(PROVIDER));
    assert_eq!(
        carried.session_id.as_deref(),
        Some(outcome.session_id.as_str())
    );
    assert_eq!(
        carried.provider_resolved_model.as_deref(),
        Some("supplied-model")
    );
    assert_eq!(carried.acknowledged_at, Some(4));
    assert!(carried.closed_at.is_none());
    assert!(!persisted.interruption.handoff_required);
    let requests = fixture.requests();
    let submissions: Vec<_> = requests
        .iter()
        .filter(|frame| frame["method"] == "submitTurn")
        .collect();
    assert_eq!(submissions.len(), 1);
    let payload = serde_json::to_string(&submissions[0]["params"]).unwrap();
    assert_eq!(payload.matches("provider-handoff").count(), 1);
    assert!(payload.contains(&attempt));
    let opens: Vec<_> = requests
        .iter()
        .filter(|frame| frame["method"] == "openThread")
        .collect();
    assert_eq!(opens.len(), 1);
    let prompt = opens[0]["params"]["instructions"].as_str().unwrap();
    let preset = crate::service::persona::resolve_ambient(
        Some(crate::multiplexer::types::AgentRole::Worker),
        None,
        None,
    )
    .unwrap();
    let contract = preset.contract_guidance();
    for fragment in [
        crate::prompts::AGENT_CODETERM_PRIMER.trim(),
        crate::prompts::AGENT_ROUTING_CONTRACT.trim(),
        crate::prompts::WORKER_CODETERM_CONTEXT.trim(),
        crate::prompts::AGENT_PAD_RULE.trim(),
        crate::prompts::RICH_RENDERING_GUIDANCE.trim(),
        crate::service::orchestrator::ROLE_AUTHORITY,
        contract.as_str(),
    ] {
        assert!(!fragment.is_empty());
        assert_eq!(prompt.matches(fragment).count(), 1, "{fragment}");
    }
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["systemPrompt"],
        prompt
    );
    assert_eq!(
        tab.restorable_launch.identity_instructions.as_deref(),
        Some(prompt)
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .boot_failure_reason
        .is_none());
}

#[test]
fn failed_target_launch_fences_the_worker_and_releases_carry_authority() {
    let (fixture, task, attempt) = switch_worker_fixture();
    let (tx, rx) = std::sync::mpsc::channel();
    let listener = fixture.ui.listen("coordination-incident", move |payload| {
        if payload["kind"] == "worker_interruption_checkpoint_failed" {
            tx.send(()).unwrap();
        }
    });
    let error = fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("refused-model".into()),
            None,
            true,
        )
        .err()
        .unwrap();
    assert!(error.contains("fixture load refused"), "{error}");
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    fixture.ui.unlisten(listener);
    let fenced = fixture.service.plan_store.show_task(&task).unwrap().task;
    assert_eq!(fenced.execution_attempts.len(), 1);
    assert_eq!(fenced.execution_attempts[0].id, attempt);
    assert!(fenced.execution_attempts[0].closed_at.is_some());
    assert_eq!(
        fenced.execution_attempts[0].interruption.reason.as_deref(),
        Some("forced_context_reset")
    );
    assert!(fenced.interruption.handoff_required);
    assert!(fenced.assigned_tab_id.is_none());
    assert!(!fixture
        .service
        .restart_carries_worker_interruption(&fixture.tab));
    assert!(!fixture
        .service
        .carry_authorized_runtime_restart_binding(&fixture.tab, "foreign-thread", "foreign-nonce")
        .unwrap());
    assert!(fixture
        .requests()
        .iter()
        .all(|frame| frame["method"] != "submitTurn"));
}

#[test]
fn failed_runtime_restart_releases_the_task_attempt_authority() {
    let (fixture, task, attempt) = switch_worker_fixture();
    fixture
        .service
        .switch_provider_in_place(
            &fixture.tab,
            PROVIDER,
            Some("supplied-model".into()),
            None,
            true,
        )
        .unwrap();
    fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .remove(&fixture.tab);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .agent_runtime_config = Some(json!({"systemPrompt":"opaque refused identity"}));
    let error = fixture
        .service
        .agent_restart_observed(&fixture.tab, true, 0)
        .unwrap_err();
    assert!(matches!(error, RestartAttemptError::Failed { .. }));
    assert!(!fixture
        .service
        .carry_authorized_runtime_restart_binding(&fixture.tab, "foreign-thread", "foreign-nonce")
        .unwrap());
    let persisted = fixture.service.plan_store.show_task(&task).unwrap().task;
    assert_eq!(persisted.execution_attempts.len(), 1);
    assert_eq!(persisted.execution_attempts[0].id, attempt);
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|frame| frame["method"] == "submitTurn")
            .count(),
        1
    );
}

fn switch_worker_fixture() -> (RuntimeFixture, String, String) {
    let fixture = RuntimeFixture::new();
    agent_runtime::engine::stop_blocking(&fixture.service, &fixture.tab).unwrap();
    let source_transcript = fixture.temp.path().join("source-thread.jsonl");
    std::fs::write(&source_transcript, []).unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session_file(
            "source-provider",
            source_transcript,
        ),
    ));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_type = Some("source-provider".into());
        tab.agent_session_id = Some("source-thread".into());
        tab.session_binding_nonce = Some("source-nonce".into());
        tab.reported_session_binding_nonce = Some("source-nonce".into());
        tab.backing = TabBacking::Pty;
        tab.pty_id = Some("source-pty-generation".into());
        tab.lifecycle_epoch = Some(1);
        tab.agent_booting = false;
    }
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "switch fixture".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque work".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task,
            &fixture.tab,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: "source-thread".into(),
                session_binding_nonce: "source-nonce".into(),
                provider: Some("source-provider".into()),
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|entry| entry.id == task)
                .unwrap();
            task.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
            task.lane.worktree_path = Some(fixture.temp.path().display().to_string());
            Ok(())
        })
        .unwrap();
    let attempt = store.show_task(&task).unwrap().task.execution_attempts[0]
        .id
        .clone();
    store
        .acknowledge_task_attempt(&plan.id, &task, &fixture.tab, &attempt, 4)
        .unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .task_binding = Some(TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task.clone()),
        attempt_id: Some(attempt.clone()),
        ..Default::default()
    });
    (fixture, task, attempt)
}

#[test]
fn loaded_runtime_supersedes_stale_boot_failure_and_can_be_repaired() {
    let fixture = RuntimeFixture::new();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        crate::service::boot_stage::apply_boot_stage(
            tab,
            codeterm_types::pty::BootStage::SpawnFailed,
            Some("provider_switch_binding_failed: opaque"),
        );
    }
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    agent_runtime::engine::publish(&fixture.service, &fixture.tab, &session);
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    assert!(tab.boot_failure_reason.is_none());
    let caller = crate::service::AuthenticatedPlanCaller {
        tab_id: Some(fixture.tab.clone()),
        session_id: tab.agent_session_id.clone(),
        binding_nonce: tab.session_binding_nonce.clone(),
    };
    let target = fixture
        .service
        .authorize_tab_repair(&fixture.tab, &caller)
        .unwrap();
    let repaired = crate::async_runtime::block_on(
        fixture
            .service
            .repair_tab_from_live_evidence(target, &caller, None),
    )
    .unwrap();
    assert_eq!(repaired.tab_id, fixture.tab);
    assert!(agent_runtime::engine::has_live_bound_session(
        fixture.service.mux.read().get_tab(&fixture.tab).unwrap()
    ));
}
