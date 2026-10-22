use super::*;

fn edit_callee(document: &mut Document, edit: impl FnOnce(&mut Document)) {
    let key = document.dependencies.keys().next().unwrap().clone();
    let mut callee = document.dependencies.remove(&key).unwrap();
    edit(&mut callee.document);
    callee.hash = blueprint_document::projection::identity(&callee.document).unwrap();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies.insert(callee.pin().key(), callee);
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
        callee.steps[1].params.insert(
            "payload".into(),
            ValueForm::Bind {
                bind: "invoke.out.n".into(),
            },
        );
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
    let mut document = chain(2);
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
