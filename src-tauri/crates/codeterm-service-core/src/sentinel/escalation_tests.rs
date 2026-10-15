use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;

use super::handlers::{Definition, Effect, Parameters, Status};
use super::machine::{self, EscalateTarget, Route, Verdict};
use super::provenance::{Classification, Failure, Scope};
use super::store::{Create, Owner, Sentinel, Store};

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

fn create_params(other: serde_json::Value) -> Create {
    let other_handlers = if other.is_array() {
        other
    } else {
        json!([other])
    };
    serde_json::from_value(json!({
        "name": "escalation-state-watch",
        "probe": "status",
        "question": "Did it finish?",
        "buckets": ["done"],
        "every": "10s",
        "timeout": "1h",
        "matches": {"done": "^done$"},
        "handlers": {
            "done": [{"id": "done_notice", "type": "notify", "parameters": {}}],
            "other": other_handlers
        },
        "escalation_timeout_ms": 5000
    }))
    .unwrap()
}

fn escalation_definition() -> serde_json::Value {
    json!({
        "id": "ask_creator",
        "type": "escalate",
        "parameters": {"target": "creator"}
    })
}

fn create_escalating(store: &mut Store) -> Sentinel {
    store
        .create(create_params(escalation_definition()), owner(), false)
        .unwrap()
}

fn scope(job: &Sentinel) -> Scope {
    Scope::from(job)
}

fn hold_other(store: &mut Store, job: &Sentinel, output: &[u8]) -> (Sentinel, String) {
    let mut evaluated = job.clone();
    let outcome = machine::evaluate_with_other(
        &mut evaluated.machine,
        output,
        &evaluated.buckets,
        &evaluated.routes,
        &evaluated.on_other,
        evaluated.until.as_deref(),
        scope(job),
        || {
            Classification::adapter(Ok(Verdict {
                bucket: "other".into(),
                confidence: 1.0,
            }))
        },
    );
    assert_eq!(outcome.reason, "other");
    evaluated.last_reason = Some(outcome.reason.into());
    let transition_id = outcome.evidence.evaluation_id;
    assert!(store
        .accept_transition(&evaluated, crate::storage::OTHER_BUCKET, &transition_id)
        .unwrap());
    (store.get(&job.id).unwrap().clone(), transition_id)
}

#[test]
fn sentinel_escalate_accepts_other_and_held_stage_skips_classification() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create_escalating(&mut store);
    let (held, transition_id) = hold_other(&mut store, &job, b"unknown");

    assert_eq!(held.machine.held.as_deref(), Some(transition_id.as_str()));
    assert_eq!(held.transitions.last().unwrap().bucket, "other");
    assert_eq!(
        held.on_other[0].action,
        Route::Escalate(EscalateTarget::Creator)
    );
    assert!(store.due().unwrap().is_empty());
    let mut held_machine = held.machine.clone();
    let outcome = machine::evaluate_with_other(
        &mut held_machine,
        b"changed",
        &held.buckets,
        &held.routes,
        &held.on_other,
        None,
        scope(&held),
        || panic!("held stage must not classify"),
    );
    assert_eq!(outcome.reason, "held");
}

#[test]
fn sentinel_escalate_low_confidence_accepts_reserved_other_transition_and_holds() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create_escalating(&mut store);
    let mut evaluated = job.clone();
    let outcome = machine::evaluate_with_other(
        &mut evaluated.machine,
        b"weak match",
        &evaluated.buckets,
        &evaluated.routes,
        &evaluated.on_other,
        evaluated.until.as_deref(),
        scope(&job),
        || {
            Classification::adapter(Ok(Verdict {
                bucket: "done".into(),
                confidence: 0.2,
            }))
        },
    );
    assert_eq!(outcome.reason, "low_confidence");
    evaluated.last_reason = Some(outcome.reason.into());
    let transition_id = outcome.evidence.evaluation_id;
    assert_eq!(
        evaluated.machine.held.as_deref(),
        Some(transition_id.as_str())
    );
    assert!(store
        .accept_transition(&evaluated, crate::storage::OTHER_BUCKET, &transition_id)
        .unwrap());
    assert_eq!(store.get(&job.id).unwrap().transitions[0].bucket, "other");
}

#[test]
fn sentinel_escalate_adapter_credit_and_budget_failures_do_not_route() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create_escalating(&mut store);
    for failure in [Failure::Disconnected, Failure::Credit, Failure::Budget] {
        let mut machine = job.machine.clone();
        let outcome = machine::evaluate_with_other(
            &mut machine,
            format!("{failure:?}").as_bytes(),
            &job.buckets,
            &job.routes,
            &job.on_other,
            None,
            scope(&job),
            || Classification::adapter(Err(failure)),
        );
        assert!(outcome.handlers.is_empty());
        assert!(machine.held.is_none());
        assert_eq!(outcome.reason, failure.reason());
    }
}

#[test]
fn sentinel_escalate_resolve_clears_hash_and_re_evaluates_once() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        clock,
    )));
    let job = create_escalating(&mut store.lock());
    let (held, transition_id) = hold_other(&mut store.lock(), &job, b"unknown");
    let prior_hash = held.machine.last_hash.clone().unwrap();
    assert!(store
        .lock()
        .stage_resolve_escalation(&job.id, &transition_id)
        .into_ready()
        .unwrap());
    assert!(store.lock().wake_stage(&job.id));
    let lease = store.lock().due().unwrap().remove(0);
    assert!(lease.job.machine.last_hash.is_none());
    assert!(lease.job.machine.held.is_none());
    assert_ne!(prior_hash, "");

    let classify_calls = std::cell::Cell::new(0);
    let execution_calls = std::cell::Cell::new(0);
    super::scheduler::advance(
        &store,
        &lease,
        &super::probe::Output {
            success: true,
            bytes: b"unknown".to_vec(),
            text: "unknown".into(),
            ..Default::default()
        },
        || {
            classify_calls.set(classify_calls.get() + 1);
            Classification::adapter(Ok(Verdict {
                bucket: "other".into(),
                confidence: 1.0,
            }))
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |_, _, _, _| {
            execution_calls.set(execution_calls.get() + 1);
            Ok(Effect {
                status: Status::Completed,
                execution_id: None,
                command_result: None,
                reason: None,
            })
        },
        |_, _, _| Ok(()),
    );
    assert_eq!(classify_calls.get(), 1);
    assert_eq!(execution_calls.get(), 1);
    let current = store.lock().get(&job.id).unwrap().clone();
    assert_eq!(current.last_reason.as_deref(), Some("other"));
    assert!(current.machine.held.is_some());
    store.lock().release(&job.id);
    assert!(store.lock().due().unwrap().is_empty());
}

#[test]
fn sentinel_escalate_expiry_persists_held_release_and_preserves_hash() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(dir.path().join("sentinels.json"), clock.clone());
    let job = create_escalating(&mut store);
    let (held, transition_id) = hold_other(&mut store, &job, b"unknown");
    let hash = held.machine.last_hash.clone();

    clock.advance_ms(4999);
    assert!(store.stage_expire_escalation().is_none());
    clock.advance_ms(1);
    let (expired_episode, staged) = store.stage_expire_escalation().unwrap();
    assert_eq!(
        expired_episode.machine.held.as_deref(),
        Some(transition_id.as_str())
    );
    assert!(staged.into_ready().unwrap());
    let expired = store.get(&job.id).unwrap();
    assert!(expired.machine.held.is_none());
    assert_eq!(expired.machine.last_hash, hash);
    assert_eq!(expired.machine.last_bucket.as_deref(), Some("other"));
    assert_eq!(expired.last_reason.as_deref(), Some("escalation_expired"));

    let mut evidence = super::provenance::Evidence::new(scope(&expired_episode), hash.clone());
    evidence.evaluation_id.clone_from(&transition_id);
    evidence.observation_id = Some(transition_id);
    let runtime = crate::service::supporter::SupporterRuntime::new(dir.path().join("supporter"));
    super::scheduler::record(
        &runtime,
        &expired_episode,
        &machine::Outcome {
            evidence,
            reason: "escalation_expired",
            verdict: None,
            route: None,
            handlers: Vec::new(),
        },
        super::provenance::Stage::EscalationExpired,
    )
    .unwrap();
    // A pattern-declared Blueprint is deterministic and never writes Supporter activity.
    assert!(!job.model_classification);
    assert!(!dir.path().join("supporter/journal.jsonl").exists());
}

#[test]
fn sentinel_escalate_claim_is_not_replayed_after_store_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let job = create_escalating(&mut store);
    let (held, transition_id) = hold_other(&mut store, &job, b"unknown");
    let claim = store
        .claim_handler(&held, &transition_id, &held.on_other[0])
        .unwrap()
        .unwrap();
    assert!(claim.claim_id.is_some());

    let mut reopened = Store::new(path, clock);
    assert!(reopened.due().unwrap().is_empty());
    let reopened_job = reopened.get(&job.id).unwrap().clone();
    assert_eq!(
        reopened_job.machine.held.as_deref(),
        Some(transition_id.as_str())
    );
    assert!(reopened_job.machine.last_hash.is_some());
    assert_eq!(reopened_job.machine.last_bucket.as_deref(), Some("other"));
    assert!(reopened
        .claim_handler(&reopened_job, &transition_id, &reopened_job.on_other[0])
        .unwrap()
        .is_none());
}

#[test]
fn sentinel_escalate_cause_cooldown_refuses_and_journals_until_five_minutes() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let store = Arc::new(Mutex::new(Store::new(
        dir.path().join("sentinels.json"),
        clock.clone(),
    )));
    let mut params = create_params(json!([
        escalation_definition(),
        {
            "id": "other_notice",
            "type": "notify",
            "parameters": {}
        }
    ]));
    params.escalation_timeout_ms = Some(600_000);
    let job = store.lock().create(params, owner(), false).unwrap();
    let first_lease = store.lock().due().unwrap().remove(0);
    let (first_hold, first_transition) =
        hold_other(&mut store.lock(), &first_lease.job, b"unknown");
    let first_claim = store
        .lock()
        .claim_handler(&first_hold, &first_transition, &first_hold.on_other[0])
        .unwrap()
        .unwrap();
    store
        .lock()
        .finish_handler(&job.id, &first_transition, &first_claim, Status::Completed)
        .unwrap();
    store.lock().release(&job.id);

    assert!(store
        .lock()
        .stage_resolve_escalation(&job.id, &first_transition)
        .into_ready()
        .unwrap());
    assert!(store.lock().wake_stage(&job.id));
    let limited_lease = store.lock().due().unwrap().remove(0);
    let classify_calls = std::cell::Cell::new(0);
    let execute_calls = std::cell::Cell::new(0);
    let journal = std::cell::RefCell::new(Vec::new());
    super::scheduler::advance(
        &store,
        &limited_lease,
        &super::probe::Output {
            success: true,
            bytes: b"unknown".to_vec(),
            text: "unknown".into(),
            ..Default::default()
        },
        || {
            classify_calls.set(classify_calls.get() + 1);
            Classification::adapter(Ok(Verdict {
                bucket: "other".into(),
                confidence: 1.0,
            }))
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |_, _, _, _| {
            execute_calls.set(execute_calls.get() + 1);
            Ok(Status::Completed.into())
        },
        |_, outcome, stage| {
            journal.borrow_mut().push((outcome.reason, stage));
            Ok(())
        },
    );
    assert_eq!(classify_calls.get(), 1);
    assert_eq!(execute_calls.get(), 1);
    assert!(journal
        .borrow()
        .contains(&("escalation_rate_limited", super::provenance::Stage::Skip)));
    let limited = store.lock().get(&job.id).unwrap().clone();
    assert!(limited.machine.held.is_some());
    let refused = limited
        .transitions
        .last()
        .unwrap()
        .handlers
        .first()
        .unwrap();
    assert_eq!(refused.status, Status::Skipped);
    assert!(refused.claim_id.is_none());
    assert_eq!(
        limited.transitions.last().unwrap().handlers[1].status,
        Status::Completed
    );
    store.lock().release(&job.id);

    clock.advance_ms(super::handlers::ESCALATION_RATE_WINDOW_MS);
    let limited_transition = limited.machine.held.unwrap();
    assert!(store
        .lock()
        .stage_resolve_escalation(&job.id, &limited_transition)
        .into_ready()
        .unwrap());
    assert!(store.lock().wake_stage(&job.id));
    let allowed_lease = store.lock().due().unwrap().remove(0);
    super::scheduler::advance(
        &store,
        &allowed_lease,
        &super::probe::Output {
            success: true,
            bytes: b"unknown".to_vec(),
            text: "unknown".into(),
            ..Default::default()
        },
        || {
            Classification::adapter(Ok(Verdict {
                bucket: "other".into(),
                confidence: 1.0,
            }))
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |_, _, _, _| Ok(Status::Completed.into()),
        |_, _, _| Ok(()),
    );
    assert_eq!(
        store
            .lock()
            .get(&job.id)
            .unwrap()
            .transitions
            .last()
            .unwrap()
            .handlers[0]
            .status,
        Status::Completed
    );
    assert_eq!(execute_calls.get(), 1);
}

#[test]
fn sentinel_escalate_forbidden_on_other_kinds_fail_create_and_amend() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let invalid = json!({
        "id": "bad_action",
        "type": "send",
        "parameters": {"target": "agent"}
    });
    assert!(store
        .prepare(create_params(invalid.clone()), owner(), false)
        .is_err());

    let job = create_escalating(&mut store);
    let update = super::amend::Update {
        model_classification: false,
        name: Some("escalation-watch".into()),
        question: job.question.clone(),
        buckets: job.buckets.clone(),
        every: Some("10s".into()),
        routes: BTreeMap::new(),
        matches: job.matches.clone(),
        rearm_on_change: Default::default(),
        until: None,
        input: None,
        handlers: None,
        on_other: vec![Definition {
            id: "bad_action".into(),
            kind: "send".into(),
            parameters: Parameters {
                target: Some("agent".into()),
                ..Default::default()
            },
        }],
        escalation_timeout_ms: Some(5000),
        max_transitions: Some(job.max_transitions),
        max_actions: Some(job.max_actions),
        cause: None,
        note: None,
    };
    assert!(update.prepare(&job, "creator", false).is_err());
}
