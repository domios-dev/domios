use super::*;
use codeterm_plugin::agent_runtime::RuntimeThreadParent;

mod restore;

#[test]
fn child_read_authority_requires_exact_epoch_nonce_root_and_recorded_child() {
    let mut ctx = RuntimeContext {
        restricted_cli: None,
        host_owned: Default::default(),
        epoch: 4,
        thread_id: Some("root".into()),
        binding_nonce: "nonce".into(),
        thread_parents: vec![RuntimeThreadParent {
            thread_id: "child".into(),
            parent_thread_id: "root".into(),
        }],
        ..Default::default()
    };
    let request = ChildHistoryRequest {
        root_thread_id: "root".into(),
        binding_nonce: "nonce".into(),
        epoch: 4,
        cursor: None,
    };
    let child = RuntimeChild {
        thread_id: "child".into(),
        parent_thread_id: "root".into(),
        ordinal: 1,
        status: "completed".into(),
        prompt: None,
        model: None,
        effort: None,
    };
    assert!(authorize(&ctx, &request, &child, "child").is_ok());
    for wrong in [
        ChildHistoryRequest {
            epoch: 3,
            ..request.clone()
        },
        ChildHistoryRequest {
            binding_nonce: "stale".into(),
            ..request.clone()
        },
        ChildHistoryRequest {
            root_thread_id: "foreign".into(),
            ..request.clone()
        },
    ] {
        assert!(authorize(&ctx, &wrong, &child, "child").is_err());
    }
    assert!(authorize(&ctx, &request, &child, "foreign").is_err());
    ctx.thread_parents.clear();
    assert!(authorize(&ctx, &request, &child, "child").is_err());
}

#[test]
fn metadata_is_epoch_scoped_and_observation_does_not_rebind_the_root() {
    let mut ctx = RuntimeContext {
        restricted_cli: None,
        thread_id: Some("root".into()),
        ..Default::default()
    };
    let mut item = ChatMsg {
        id: "spawn".into(),
        tool_kind: Some("agent".into()),
        tool_name: Some("agent_activity".into()),
        tool_input: Some(
            serde_json::json!({"agent_id":"child","agent_parent_thread_id":"root","agent_ordinal":1,"agent_state":"started"}),
        ),
        ..Default::default()
    };
    let mut maps = super::super::events::RuntimeMaps::default();
    let event = super::super::events::RuntimeEvent::ItemUpdated {
        thread_id: "root".into(),
        item: item.clone(),
    };
    super::super::events::fold(&mut maps, 4, &event);
    super::super::events::observe(&mut ctx, &event);
    assert_eq!(ctx.thread_id.as_deref(), Some("root"));
    assert!(ctx.active_turns.is_empty());
    assert!(maps.child(4, "child").is_some());
    assert!(maps.child(5, "child").is_none());
    assert!(runtime_descendant(&ctx, "child"));
    item.tool_input
        .as_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("agent_parent_thread_id");
    assert!(codeterm_plugin::agent_runtime::runtime_child("root", &item).is_none());
}

#[test]
fn history_restores_only_own_root_relations_without_claiming_liveness() {
    let mut context = RuntimeContext {
        restricted_cli: None,
        host_owned: Default::default(),
        epoch: 2,
        thread_id: Some("root".into()),
        binding_nonce: "new".into(),
        ..Default::default()
    };
    let mut maps = events::RuntimeMaps::default();
    let row = |parent: &str| ChatMsg {
        id: parent.into(),
        tool_kind: Some("agent".into()),
        tool_name: Some("agent_activity".into()),
        tool_input: Some(
            serde_json::json!({"agent_id":"child", "agent_parent_thread_id":parent, "agent_ordinal":1, "agent_state":"running", "agent_binding_nonce":"old", "agent_runtime_epoch":1}),
        ),
        ..Default::default()
    };
    restore_relations(&mut context, &mut maps, "foreign", &[row("root")]);
    restore_relations(&mut context, &mut maps, "root", &[row("foreign")]);
    assert!(maps.child(2, "child").is_none());
    restore_relations(&mut context, &mut maps, "root", &[row("root")]);
    let child = maps.child(2, "child").unwrap();
    assert_eq!(child.status, "unknown");
    assert!(maps.child(1, "child").is_none());
    assert!(context.active_turns.is_empty());
    let request = ChildHistoryRequest {
        root_thread_id: "root".into(),
        binding_nonce: "new".into(),
        epoch: 2,
        cursor: None,
    };
    assert!(authorize(&context, &request, &child, "child").is_ok());
    assert!(authorize(
        &context,
        &ChildHistoryRequest {
            epoch: 1,
            ..request
        },
        &child,
        "child"
    )
    .is_err());
}
