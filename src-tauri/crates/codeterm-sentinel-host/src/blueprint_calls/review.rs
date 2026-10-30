use super::*;

#[test]
fn blueprint_v2_call_retained_failure_survives_unchanged_decision() {
    for parent in [serde_json::Value::Null, json!(0)] {
        let entries = [
            json!({"entry_id":3,"run_id":"run","step_index":1,"parent_step_index":parent,"step_name":"command","state":"done","outcome":"unchanged","started_at_ms":2,"ended_at_ms":2,"reason":null}),
            json!({"entry_id":2,"run_id":"run","step_index":1,"parent_step_index":parent,"step_name":"command","state":"failed","outcome":"failed","started_at_ms":1,"ended_at_ms":2,"reason":"deadline"}),
        ].into_iter().map(|value| serde_json::from_value(value).unwrap()).collect::<Vec<_>>();
        let projection = project_retained(None, &entries, None, &Default::default()).unwrap();
        let nodes = projection["nested_run"]["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["state"], "failed");
        assert_eq!(nodes[0]["reason"], "deadline");
        assert_eq!(nodes[0]["parent_step_index"], parent);
    }
}

fn edit_callee(document: &mut Document, edit: impl FnOnce(&mut Document)) {
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    edit(&mut callee.document);
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
}

#[test]
fn blueprint_v2_call_portable_python_main_returns_eight_through_claimed_effect() {
    let mut document = source(false);
    edit_callee(&mut document, |callee| {
        let node = &mut callee.steps[0];
        node.run = None;
        node.code = Some(
            serde_json::from_value(json!({
                "lang":"python", "src":"def main(value): return {'n': value['n'] + 1}"
            }))
            .unwrap(),
        );
    });
    let receipt = run(document, false);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    assert_eq!(receipt.notices, ["{\"n\":8}"]);
    assert!(receipt.stop.is_none());
    assert!(receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"
            && row.state == "done"
            && !row.call_stack.is_empty()));
}

#[test]
fn blueprint_v2_call_review_other_refuses_before_success_path_effects() {
    let mut document = source(false);
    edit_callee(&mut document, |callee| {
        callee.other.push(callee.steps[1].clone());
        callee.other[0].id = "failure-only".into();
    });
    let receipt = run(document, false);
    assert_eq!(
        receipt.output.unwrap_err(),
        JournalReason::RuntimeUnavailable
    );
    assert!(receipt.notices.is_empty());
    assert!(!receipt
        .journal
        .iter()
        .any(|row| row.step_name == "increment"));
}

#[test]
fn blueprint_v2_call_review_simulate_refuses_process_without_delivery_or_real_journal() {
    let receipt = run(source(false), true);
    assert_eq!(
        receipt.output.unwrap_err(),
        JournalReason::RuntimeUnavailable
    );
    assert!(receipt.notices.is_empty());
    assert!(receipt.effects.is_empty());
    assert!(receipt.journal.is_empty());
}

#[test]
fn blueprint_v2_call_review_simulate_pure_nested_graph_writes_no_real_journal() {
    let receipt = run(chain(2), true);
    assert_eq!(receipt.output.unwrap().value, json!({"n":7}));
    assert!(receipt.notices.is_empty());
    assert!(receipt.journal.is_empty());
}

#[test]
fn blueprint_v2_call_review_same_named_nodes_have_authoritative_parent_and_version() {
    let mut document = source(false);
    edit_callee(&mut document, |callee| {
        callee.steps[0].id = "invoke".into();
        callee.result.from = Some("invoke.out".into());
    });
    let receipt = run(document, false);
    assert_eq!(receipt.output.unwrap().value, json!({"n":8}));
    let rows = receipt
        .journal
        .iter()
        .filter(|row| row.step_name == "invoke" && row.state == JournalState::Done)
        .map(|row| json!(row))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    let root = rows.iter().find(|row| row["step_index"] == 0).unwrap();
    let child = rows.iter().find(|row| row["step_index"] == 1).unwrap();
    assert_eq!(root["parent_step_index"], Value::Null);
    assert_eq!(child["parent_step_index"], 0);
    assert_eq!(root["definition_version"], 1);
    assert_eq!(child["definition_version"], 1);
}

#[test]
fn blueprint_v2_call_review_nested_callable_check_preserves_signature_and_stays_inert() {
    use crate::BlueprintApplyExt;
    let root = tempfile::tempdir().unwrap();
    let (mut mux, creator) = MultiplexerState::new();
    mux.resolve_or_mint_team_for_parent(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let service = &instance.fixture.service;
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(
            &instance.storage,
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        ),
        service.ui.clone(),
    );
    crate::manage_fixture_ports(service, &handle);
    let caller = service.sentinel_caller(&creator, None).unwrap();
    let mut document = source(false);
    document.trigger = None;
    document.result.to = None;
    document.result.port = Some(serde_json::from_value(port()).unwrap());
    document.result.from = Some("invoke.out".into());
    document
        .inputs
        .insert("p".into(), serde_json::from_value(port()).unwrap());
    document.steps[0]
        .call
        .as_mut()
        .unwrap()
        .arguments
        .insert("p".into(), ValueForm::input("p"));
    let result = service
        .blueprint_apply(&caller, json!(document), true)
        .unwrap();
    assert_eq!(result["after"]["inputs"]["p"], port());
    assert!(handle.store.lock().jobs.is_empty());
    assert!(service.notif.lock().list(false).is_empty());
    drop(handle);
    close(instance);
}

#[test]
fn blueprint_v2_call_review_simulate_refuses_code_agent_and_http_before_launch() {
    for body in [
        json!({"id":"increment","code":{"lang":"python","src":"def main(value):\n raise RuntimeError('must not execute')"}}),
        json!({"id":"increment","uses":"agent","with":{"task":{"lit":"must not execute"}}}),
        json!({"id":"increment","uses":"http","with":{"url":{"lit":"https://must-not-execute.invalid"}}}),
    ] {
        let mut document = source(false);
        edit_callee(&mut document, |callee| {
            callee.steps[0] = serde_json::from_value(body).unwrap();
            if let Some(code) = &callee.steps[0].code {
                assert!(blueprint_document::code_contract::has_main(code));
            }
        });
        let receipt = run(document, true);
        assert_eq!(
            receipt.output.unwrap_err(),
            JournalReason::RuntimeUnavailable
        );
        assert!(receipt.notices.is_empty());
        assert!(receipt.effects.is_empty());
        assert!(receipt.journal.is_empty());
        assert!(
            receipt.child_pids.is_empty(),
            "simulation must never launch a child"
        );
    }
}

#[test]
fn blueprint_v2_call_review_import_export_update_preserves_pins_and_version_history() {
    use crate::{BlueprintApplyExt, BlueprintPortableExt};
    use codeterm_blueprint_types::blueprint_recipe::Recipe;
    let root = tempfile::tempdir().unwrap();
    let (mut mux, creator) = MultiplexerState::new();
    mux.resolve_or_mint_team_for_parent(&creator).unwrap();
    mux.get_tab_mut(&creator).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let service = &instance.fixture.service;
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(
            &instance.storage,
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        ),
        service.ui.clone(),
    );
    crate::manage_fixture_ports(service, &handle);
    let caller = service.sentinel_caller(&creator, None).unwrap();
    let document = source(false);
    let pin = document.steps[0].call.as_ref().unwrap().pin.clone();
    let imported = service
        .blueprint_import(
            &caller,
            Recipe {
                schema_version: 2,
                definition: json!(document),
            },
            &[],
            true,
        )
        .unwrap();
    let id = imported["id"].as_str().unwrap();
    let mut exported = service.blueprint_export(&caller, id).unwrap();
    exported.definition["note"] = json!("Amend metadata while retaining the exact call pin");
    assert_eq!(exported.definition["steps"][0]["call"]["pin"], json!(pin));
    let update = service
        .blueprint_apply_document(
            &caller,
            exported.definition,
            false,
            None,
            Some((id.into(), 1, None)),
        )
        .unwrap();
    assert_eq!(update["definition_version"], 2);
    let stored = handle.store.lock().get(id).unwrap().clone();
    assert_eq!(stored.definition_version, 2);
    assert_eq!(
        stored.document.unwrap()["steps"][0]["call"]["pin"],
        json!(pin)
    );
    assert_eq!(handle.store.lock().jobs.len(), 1);
    assert!(service.notif.lock().list(false).is_empty());
    drop(handle);
    close(instance);
}

#[test]
fn blueprint_v2_call_retained_show_never_borrows_current_pins_for_unknown_history() {
    let entries = vec![serde_json::from_value(json!({"entry_id":1,"run_id":"old-run","step_index":0,"step_name":"invoke","state":"done","outcome":"done","started_at_ms":1,"ended_at_ms":2,"result_summary":null,"reason":null,"body_ref":null})).unwrap()];
    let view = project_retained(None, &entries, None, &Default::default()).unwrap();
    assert_eq!(view["historical_graph_available"], false);
    assert_eq!(view["expanded_calls"], json!([]));
    assert_eq!(view["call_graph"], Value::Null);
    assert_eq!(view["nested_run"]["nodes"][0]["step_name"], "invoke");
    assert_eq!(view["graph_definition_version"], Value::Null);
}

#[test]
fn blueprint_v2_call_retained_show_preserves_exact_historical_body_and_version() {
    let historical = source(false);
    let pin = historical.steps[0]
        .call
        .as_ref()
        .unwrap()
        .pin
        .as_ref()
        .unwrap();
    let entries = vec![serde_json::from_value(json!({"entry_id":1,"run_id":"old-run","step_index":0,"step_name":"invoke","state":"done","outcome":"done","started_at_ms":1,"ended_at_ms":2,"result_summary":null,"reason":null,"body_ref":null,"definition_version":1})).unwrap()];
    let view = project_retained(Some(&historical), &entries, Some(1), &Default::default()).unwrap();
    assert_eq!(view["historical_graph_available"], true);
    assert_eq!(view["graph_definition_version"], 1);
    assert_eq!(view["expanded_calls"][0]["pin"], json!(pin));
    assert_eq!(
        view["expanded_calls"][0]["graph"]["steps"][0]["run"],
        json!(historical.dependencies[&pin.key()].document.steps[0].run)
    );
}

#[test]
fn blueprint_stored_identity_1152_applied_callee_executes_and_projects_recorded_hash() {
    let fixtures: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/blueprint-identities-1.15.2.golden.json"
    )))
    .unwrap();
    let fixture = fixtures
        .as_array()
        .unwrap()
        .iter()
        .find(|fixture| fixture["name"] == "call")
        .unwrap();
    let mut document: Document = serde_json::from_value(fixture["document"].clone()).unwrap();
    let mut callee = std::mem::take(&mut document.dependencies)
        .into_values()
        .next()
        .unwrap();
    callee.id = "inline:legacy#helper".into();
    let recorded = callee.hash.clone();
    document
        .dependencies
        .insert(callee.pin().key(), callee.clone());
    let call = document.steps[0].call.as_mut().unwrap();
    call.reference = "helper".into();
    call.pin = Some(callee.pin());
    document.owner = Some("tab".into());
    let receipt = run(document, false);
    assert_eq!(
        receipt.output.unwrap().value.as_str().unwrap().trim(),
        "ready"
    );
    assert!(receipt.stop.is_none());
    assert!(receipt
        .journal
        .iter()
        .any(|row| row.call_stack.iter().any(|frame| frame.hash == recorded)));
    assert_eq!(receipt.view["expanded_calls"][0]["pin"]["hash"], recorded);
}
