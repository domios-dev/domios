use super::*;

#[test]
fn blueprint_handlers_round_trip_without_changing_stored_identity_or_content() {
    let public = json!({
        "id": "sentinel_existing", "probe": "sentinel is opaque probe content",
        "routes": {"done": "blueprint:sentinel_child"},
        "handlers": {"done": [{"id":"join", "type":"blueprint", "parameters":{"target":"sentinel_child"}}]}
    });
    let stored = stored_definition(public.clone()).unwrap();
    assert_eq!(stored["id"], "sentinel_existing");
    assert_eq!(stored["probe"], public["probe"]);
    assert_eq!(stored["routes"]["done"], "blueprint:sentinel_child");
    assert_eq!(stored["handlers"]["done"][0]["type"], "blueprint");
    assert_eq!(exposed_definition(stored), public);
    let persisted = json!({"routes":{"done":[{"id":"join","action":{"kind":"sentinel","target":"sentinel_child"},"target":{"kind":"sentinel","id":"sentinel_child"}}]}});
    let exposed = exposed_definition(persisted.clone());
    assert_eq!(exposed["routes"]["done"][0]["action"]["kind"], "blueprint");
    assert_eq!(exposed["routes"]["done"][0]["target"]["kind"], "blueprint");
    assert_eq!(stored_definition(persisted).unwrap(), exposed);
}

#[test]
fn blueprint_handlers_accept_legacy_public_names() {
    assert!(stored_definition(json!({"routes":{"done":"sentinel:child"}} )).is_ok());
    assert!(stored_definition(
        json!({"handlers":{"done":[{"type":"sentinel","parameters":{"target":"child"}}]}})
    )
    .is_ok());
}

#[test]
fn sentinel_update_rpc_version_conflict_carries_current_version() {
    let error = error(codeterm_sentinel::amend::version_conflict(9));
    assert_eq!(
        error.data.as_ref().unwrap()["code"],
        "blueprint_version_conflict"
    );
    assert_eq!(error.data.as_ref().unwrap()["current_version"], 9);
}

#[test]
fn blueprint_name_conflict_rpc_preserves_typed_code_and_recovery_message() {
    let refusal = "blueprint_name_conflict: Choose a content-describing name, for example increment-seventeen";
    let failure = error(refusal);
    assert_eq!(failure.code, -32602);
    assert_eq!(
        failure.data,
        Some(json!({"code":"blueprint_name_conflict"}))
    );
    assert_eq!(failure.message, refusal);
}
