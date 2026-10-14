use super::machine::{self, Route};
use super::provenance::{Classification, Scope};
use super::store::{Create, Owner, Store};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

fn definition() -> Value {
    json!({
        "name": "handler-result-watch",
        "probe": "status", "question": "state?", "buckets": ["done"],
        "every": "10s", "matches": {"done": "^done$"}, "timeout": "1h"
    })
}

#[test]
fn sentinel_none_route_has_no_handler_and_does_not_fall_back_to_notify() {
    let routes = super::handlers::definitions(
        &["quiet".into()],
        BTreeMap::from([("quiet".into(), "none".into())]),
        None,
    )
    .unwrap();

    assert!(routes["quiet"].is_empty());
}

#[test]
fn sentinel_handlers_current_create_evaluate_and_reload_positive() {
    let dir = tempfile::tempdir().unwrap();
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(path.clone(), clock.clone());
    let mut job = store
        .create(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    assert_eq!(Route::parse("notify").unwrap(), Route::Notify);
    let scope = Scope::from(&job);
    let outcome = machine::evaluate(
        &mut job.machine,
        b"done",
        &job.buckets,
        &job.routes,
        None,
        scope,
        || {
            Classification::regex(machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
    );
    assert_eq!(outcome.reason, "transition");
    assert!(store.finish(&job).unwrap());
    let restored = Store::new(path, clock);
    assert_eq!(
        restored
            .get(&job.id)
            .unwrap()
            .machine
            .last_bucket
            .as_deref(),
        Some("done")
    );
}

#[test]
fn sentinel_handlers_paused_creation_is_persisted_and_never_due() {
    let dir = tempfile::tempdir().unwrap();
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(path.clone(), clock.clone());
    let mut value = definition();
    value["paused"] = json!(true);
    let params: Create = serde_json::from_value(value).expect("paused definition must be admitted");
    let job = store.create(params, owner(), false).unwrap();
    let mut paused = job.machine.clone();
    let outcome = machine::evaluate(
        &mut paused,
        b"done",
        &job.buckets,
        &job.routes,
        None,
        Scope::from(&job),
        || panic!("paused nodes must not consume Choice"),
    );
    assert_eq!(outcome.reason, "paused");
    assert!(paused.last_hash.is_none());
    assert!(store.due().unwrap().is_empty());
    assert_eq!(
        serde_json::to_value(store.get(&job.id).unwrap()).unwrap()["machine"]["paused"],
        true
    );
    assert!(Store::new(path, clock).due().unwrap().is_empty());
}

#[test]
fn sentinel_handlers_declared_array_roundtrips_order_and_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let mut value = definition();
    value["handlers"] = json!({"done": [
        {"id": "notice", "type": "notify", "parameters": {}},
        {"id": "finish", "type": "stop", "parameters": {}}
    ]});
    let params: Create = serde_json::from_value(value).expect("declared handlers must be admitted");
    let job = store.create(params, owner(), false).unwrap();
    let stored = serde_json::to_value(&job).unwrap();
    assert_eq!(stored["routes"]["done"][0]["id"], "notice");
    assert_eq!(stored["routes"]["done"][1]["id"], "finish");
}

#[test]
fn sentinel_handlers_exact_sentinel_route_is_a_supported_closed_action() {
    assert!(Route::parse("notify").is_ok());
    assert!(Route::parse("sentinel:sentinel_exact").is_ok());
    for value in [
        "command:echo",
        "sentinel:",
        "sentinel:peer/remote",
        "sentinel:peer::remote",
    ] {
        assert!(Route::parse(value).is_err(), "{value}");
    }
}

#[test]
fn sentinel_respond_prompt_route_and_handlers_file_parse_safe_kinds() {
    assert_eq!(
        Route::parse("respond_prompt:onboarding").unwrap(),
        Route::RespondPrompt(super::machine::SafeKind::Onboarding)
    );
    let definitions = BTreeMap::from([(
        "done".into(),
        vec![super::handlers::Definition {
            id: "answer".into(),
            kind: "respond_prompt".into(),
            parameters: super::handlers::Parameters {
                target: Some("permission".into()),
                ..Default::default()
            },
        }],
    )]);
    let handlers =
        super::handlers::definitions(&["done".into()], BTreeMap::new(), Some(definitions)).unwrap();
    assert_eq!(
        handlers["done"][0].action,
        Route::RespondPrompt(super::machine::SafeKind::Permission)
    );
}

#[test]
fn sentinel_respond_prompt_shell_probe_source_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let mut value = definition();
    value["routes"] = json!({"done": "respond_prompt:onboarding"});
    let params: Create = serde_json::from_value(value).unwrap();
    assert!(store
        .create(params, owner(), false)
        .unwrap_err()
        .contains("tab observation source"));
}

fn transition(store: &mut Store, job: &super::store::Sentinel) -> (super::store::Sentinel, String) {
    let mut accepted = job.clone();
    let outcome = machine::evaluate(
        &mut accepted.machine,
        b"done",
        &accepted.buckets,
        &accepted.routes,
        None,
        Scope::from(job),
        || {
            Classification::regex(machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
    );
    let id = outcome.evidence.evaluation_id;
    assert!(store.accept_transition(&accepted, "done", &id).unwrap());
    (accepted, id)
}

#[test]
fn sentinel_handlers_claim_reload_never_replays_unconfirmed_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let job = store
        .create(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let (job, transition) = transition(&mut store, &job);
    let handler = &job.routes["done"][0];
    let claim = store
        .claim_handler(&job, &transition, handler)
        .unwrap()
        .unwrap();
    assert!(claim.claim_id.is_some());
    assert!(claim.execution_id.is_none());
    drop(store);
    let mut restored = Store::new(path, clock);
    assert!(restored
        .claim_handler(&job, &transition, handler)
        .unwrap()
        .is_none());
    assert_eq!(
        restored.get(&job.id).unwrap().transitions[0].handlers[0].status,
        super::handlers::Status::Unconfirmed
    );
}

#[test]
fn sentinel_handlers_paused_expiry_preserves_creation_lifetime() {
    let dir = tempfile::tempdir().unwrap();
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(dir.path().join("sentinels.json"), clock.clone());
    let mut value = definition();
    value["paused"] = json!(true);
    value["timeout"] = json!("10s");
    let job = store
        .create(serde_json::from_value(value).unwrap(), owner(), false)
        .unwrap();
    assert_eq!(store.next_deadline(), 10_000);
    clock.advance_ms(10_000);
    assert!(store.due().unwrap().is_empty());
    let current = store.get(&job.id).unwrap();
    assert!(current.machine.stopped);
    assert_eq!(current.last_reason.as_deref(), Some("timeout"));
}

#[test]
fn sentinel_handlers_activation_refuses_exact_scope_and_terminal_targets() {
    let dir = tempfile::tempdir().unwrap();
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(dir.path().join("sentinels.json"), clock.clone());
    let source = store
        .create(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let mut paused = definition();
    paused["paused"] = json!(true);
    let target = store
        .create(
            serde_json::from_value(paused.clone()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    assert!(store.validate_target(&source, &target.id).is_ok());
    assert!(store.validate_target(&source, &source.id).is_err());
    assert!(store.validate_target(&source, "missing").is_err());
    for changed in [
        Owner {
            creator_tab_id: "foreign".into(),
            ..owner()
        },
        Owner {
            team_id: None,
            ..owner()
        },
        Owner {
            workspace_id: "foreign".into(),
            ..owner()
        },
    ] {
        let foreign = store
            .create(
                serde_json::from_value(paused.clone()).unwrap(),
                changed,
                false,
            )
            .unwrap();
        assert!(store.validate_target(&source, &foreign.id).is_err());
    }
    store.stop(&target.id, "explicit").unwrap();
    assert!(store.validate_target(&source, &target.id).is_err());
    paused["timeout"] = json!("10s");
    let expired = store
        .create(serde_json::from_value(paused).unwrap(), owner(), false)
        .unwrap();
    clock.advance_ms(10_000);
    assert!(store.validate_target(&source, &expired.id).is_err());
}

#[test]
fn sentinel_handlers_claim_and_result_save_failures_preserve_durable_claim() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let job = store
        .create(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let (job, transition) = transition(&mut store, &job);
    let handler = &job.routes["done"][0];
    std::fs::rename(&path, dir.path().join("saved.json")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.claim_handler(&job, &transition, handler).is_err());
    assert!(store.get(&job.id).unwrap().transitions[0]
        .handlers
        .is_empty());
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(dir.path().join("saved.json"), &path).unwrap();
    let mut store = Store::new(path.clone(), clock.clone());
    let claim = store
        .claim_handler(&job, &transition, handler)
        .unwrap()
        .unwrap();
    std::fs::rename(&path, dir.path().join("saved.json")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store
        .finish_handler(
            &job.id,
            &transition,
            &claim,
            super::handlers::Status::Completed
        )
        .is_err());
    assert_eq!(
        store.get(&job.id).unwrap().transitions[0].handlers[0].status,
        super::handlers::Status::Unconfirmed
    );
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(dir.path().join("saved.json"), &path).unwrap();
    let mut restored = Store::new(path, clock);
    assert!(restored
        .claim_handler(&job, &transition, handler)
        .unwrap()
        .is_none());
}

#[test]
fn sentinel_handlers_definition_validation_and_budgets_are_finite() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    for handlers in [
        json!({"done":[{"id":"h","type":"notify","parameters":{}},{"id":"h","type":"stop","parameters":{}}]}),
        json!({"undeclared":[{"id":"h","type":"notify","parameters":{}}]}),
        json!({"done":[{"id":"h","type":"command","parameters":{}}]}),
        json!({"done":[{"id":"h","type":"unknown","parameters":{}}]}),
    ] {
        let mut value = definition();
        value["handlers"] = handlers;
        assert!(store
            .prepare(serde_json::from_value(value).unwrap(), owner(), false)
            .is_err());
    }
    for field in ["max_transitions", "max_actions"] {
        let mut value = definition();
        value[field] = json!(0);
        assert!(store
            .prepare(serde_json::from_value(value).unwrap(), owner(), false)
            .is_err());
    }
    let mut value = definition();
    value["max_actions"] = json!(1);
    value["max_transitions"] = json!(1);
    let job = store
        .create(serde_json::from_value(value).unwrap(), owner(), false)
        .unwrap();
    let (accepted, id) = transition(&mut store, &job);
    let claim = store
        .claim_handler(&accepted, &id, &accepted.routes["done"][0])
        .unwrap()
        .unwrap();
    store
        .finish_handler(&job.id, &id, &claim, super::handlers::Status::Completed)
        .unwrap();
    let mut later = accepted.clone();
    later.machine.last_bucket = None;
    assert!(!store
        .accept_transition(&later, "done", "new-transition")
        .unwrap());
    assert_eq!(
        store.get(&job.id).unwrap().last_reason.as_deref(),
        Some("transition_budget_exhausted")
    );
}

#[test]
fn sentinel_handlers_active_and_total_caps_include_dormant_inventory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let seed = Store::new(path.clone(), clock.clone());
    let mut jobs = Vec::new();
    for index in 0..256 {
        let mut value = definition();
        value["paused"] = json!(index >= 64);
        jobs.push(
            seed.prepare(serde_json::from_value(value).unwrap(), owner(), false)
                .unwrap(),
        );
    }
    std::fs::write(&path, serde_json::to_vec(&jobs[..64]).unwrap()).unwrap();
    let mut store = Store::new(path.clone(), clock.clone());
    assert!(store
        .prepare(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false
        )
        .is_err());
    let mut paused = definition();
    paused["paused"] = json!(true);
    assert!(store
        .create(
            serde_json::from_value(paused.clone()).unwrap(),
            owner(),
            false
        )
        .is_ok());
    std::fs::write(&path, serde_json::to_vec(&jobs).unwrap()).unwrap();
    let store = Store::new(path, clock);
    assert_eq!(store.list().len(), 256);
    assert!(store
        .prepare(serde_json::from_value(paused).unwrap(), owner(), false)
        .is_err());
}

#[test]
fn sentinel_handlers_unsupported_and_corrupt_stores_preserve_bytes_and_refuse_work() {
    for raw in [
        r#"[{"id":"old","routes":{"done":{"kind":"notify"}}}]"#,
        "{invalid-json",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sentinels.json");
        std::fs::write(&path, raw).unwrap();
        let mut store = Store::new(
            path.clone(),
            codeterm_cron::clock::MockClock::starting_at(1000),
        );
        let error = store.ensure_available().unwrap_err();
        assert!(error.starts_with(if raw.contains("routes") {
            "blueprint_store_unsupported_format:"
        } else {
            "blueprint_store_load_failed:"
        }));
        assert!(store
            .create(
                serde_json::from_value(definition()).unwrap(),
                owner(),
                false
            )
            .is_err());
        assert!(store.stop("old", "explicit").is_err());
        assert!(store.due().is_err());
        assert!(store.take_changes().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), raw.as_bytes());
    }
}

#[test]
fn sentinel_handlers_io_failure_and_corruption_after_load_hold_the_original_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    std::fs::create_dir(&path).unwrap();
    let mut unavailable = Store::new(
        path.clone(),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    assert!(unavailable
        .ensure_available()
        .unwrap_err()
        .starts_with("blueprint_store_load_failed:"));
    assert!(unavailable.due().is_err());
    assert!(path.is_dir());
    std::fs::remove_dir(&path).unwrap();
    crate::persisted_state::release_hold(&path);
    let mut store = Store::new(
        path.clone(),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    assert!(store.ensure_available().is_ok());
    let job = store
        .create(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let corrupt = b"{externally-corrupted";
    std::fs::write(&path, corrupt).unwrap();
    assert!(store.stop(&job.id, "explicit").is_err());
    assert!(store.due().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), corrupt);
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("sentinels.json.corrupt")));
}

#[test]
fn sentinel_handlers_real_execution_id_is_preserved_independently_from_durable_claim() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let job = store
        .create(
            serde_json::from_value(definition()).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let (job, transition) = transition(&mut store, &job);
    let handler = &job.routes["done"][0];
    let claim = store
        .claim_handler(&job, &transition, handler)
        .unwrap()
        .unwrap();
    store
        .finish_handler_result(
            &job.id,
            &transition,
            &claim,
            super::handlers::Effect {
                status: super::handlers::Status::Completed,
                execution_id: Some("supplied-real-run-id".into()),
                command_result: None,
                reason: None,
            },
        )
        .unwrap();
    let mut restored = Store::new(path, clock);
    let result = &restored.get(&job.id).unwrap().transitions[0].handlers[0];
    assert_eq!(result.claim_id, claim.claim_id);
    assert_eq!(result.execution_id.as_deref(), Some("supplied-real-run-id"));
    assert!(restored
        .claim_handler(&job, &transition, handler)
        .unwrap()
        .is_none());
}

#[test]
fn sentinel_handlers_array_limits_unknown_fields_and_long_bucket_ids_are_checked_by_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_cron::clock::MockClock::starting_at(1000),
    );
    let mut value = definition();
    value["handlers"] = json!({"done":(0..17).map(|index| json!({"id":format!("h_{index}"),"type":"notify","parameters":{}})).collect::<Vec<_>>()});
    assert!(store
        .prepare(serde_json::from_value(value).unwrap(), owner(), false)
        .is_err());
    for handler in [
        json!({"id":"h","type":"notify","parameters":{},"unknown":true}),
        json!({"id":"h","type":"notify","parameters":{"unknown":true}}),
    ] {
        let mut value = definition();
        value["handlers"] = json!({"done":[handler]});
        assert!(serde_json::from_value::<Create>(value).is_err());
    }
    let bucket = "x".repeat(64);
    let mut value = definition();
    value["buckets"] = json!([bucket]);
    value["matches"] = json!({(bucket.clone()):"done"});
    assert!(store
        .prepare(serde_json::from_value(value).unwrap(), owner(), false)
        .is_ok());
}

#[test]
fn sentinel_handlers_activation_requires_durable_declared_claim_and_checks_active_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let seed = Store::new(path.clone(), clock.clone());
    let mut jobs: Vec<_> = (0..65)
        .map(|index| {
            let mut value = definition();
            value["paused"] = json!(index == 64);
            seed.prepare(serde_json::from_value(value).unwrap(), owner(), false)
                .unwrap()
        })
        .collect();
    let target = jobs[64].id.clone();
    jobs[0].routes.get_mut("done").unwrap()[0].action = Route::Sentinel(target.clone());
    std::fs::write(&path, serde_json::to_vec(&jobs).unwrap()).unwrap();
    let mut store = Store::new(path, clock);
    let source = store.get(&jobs[0].id).unwrap().clone();
    let (source, id) = transition(&mut store, &source);
    assert!(store.activate(&source, &id, &target).is_err());
    store
        .claim_handler(&source, &id, &source.routes["done"][0])
        .unwrap()
        .unwrap();
    assert!(store
        .activate(&source, &id, &target)
        .unwrap_err()
        .contains("capacity"));
    assert!(store.get(&target).unwrap().machine.paused);
}

#[test]
fn sentinel_causal_run_json_activation_and_downstream_accept_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let mut child_definition = definition();
    child_definition["paused"] = json!(true);
    let child = store
        .create(
            serde_json::from_value(child_definition).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let mut parent_definition = definition();
    parent_definition["handlers"] =
        json!({"done":[{"id":"child","type":"blueprint","parameters":{"target":child.id}}]});
    let parent = store
        .create(
            serde_json::from_value(parent_definition).unwrap(),
            owner(),
            false,
        )
        .unwrap();
    let (parent, id) = transition(&mut store, &parent);
    store
        .claim_handler(&parent, &id, &parent.routes["done"][0])
        .unwrap()
        .unwrap();
    assert!(store.activate(&parent, &id, &child.id).unwrap());
    let context = store.get(&child.id).unwrap().causal_run.clone().unwrap();
    assert_eq!(context.parent.as_ref().unwrap().node, parent.id);
    assert_eq!(context.parent.as_ref().unwrap().transition, id);
    drop(store);
    let mut store = Store::new(path, clock);
    let child = store.get(&child.id).unwrap().clone();
    assert_eq!(child.causal_run.as_ref(), Some(&context));
    transition(&mut store, &child);
    assert_eq!(
        store.get(&child.id).unwrap().transitions[0]
            .causal_run
            .as_ref(),
        Some(&context)
    );
    assert_eq!(
        store.get(&parent.id).unwrap().transitions[0]
            .causal_run
            .as_ref()
            .unwrap()
            .id,
        context.id
    );
}
