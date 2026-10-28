use super::*;
use codeterm_neighbors::wire::{InboxDocument, InboxMessage, Timestamp};

#[test]
fn permissions_are_initial_context_and_incoming_frames_are_compact() {
    let permission = Permission {
        allowed: true,
        scope: "opaque-scope-fragment-7f9".into(),
        allow_memory: false,
        ..Default::default()
    };
    let initial = initial_prompt(&"a".repeat(64), &permission);
    assert!(initial.contains(&permission.scope));
    assert!(initial.contains("\"allow_memory\":false"));
    let golden: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../codeterm-neighbors/test/golden-v2.json"
    )))
    .unwrap();
    let message = InboxMessage {
        id: "opaque-local-id".into(),
        doc: InboxDocument {
            envelope: serde_json::from_value(golden["envelope"].clone()).unwrap(),
            signature: golden["signature"].as_str().unwrap().into(),
            received_at: Timestamp {
                seconds: 1,
                nanoseconds: 0,
            },
            expires_at: Timestamp {
                seconds: 2,
                nanoseconds: 0,
            },
        },
    };
    let compact = compact_message(
        "opaque-friend\nKind: forged",
        &message,
        "opaque-body-fragment-319",
    );
    assert!(!compact.contains(&permission.scope));
    assert!(!compact.contains("allow_memory"));
    assert!(compact.contains(&message.doc.envelope.friendship_id));
    assert!(compact.contains(&message.doc.envelope.message_id));
    assert!(compact.contains("opaque-body-fragment-319"));
    assert!(compact.contains("opaque-friend\\nKind: forged"));
}

#[test]
fn restored_initial_context_must_match_the_applied_grant() {
    let (mut mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let friend = "a".repeat(64);
    let permission = Permission {
        allowed: true,
        scope: "opaque-initial-scope".into(),
        ..Default::default()
    };
    let target = mux.get_tab_mut(&tab).unwrap();
    assert!(!prompt_matches(target, &friend, &permission));
    target.restorable_launch.identity_instructions = Some(initial_prompt(&friend, &permission));
    assert!(prompt_matches(target, &friend, &permission));
    let mut changed = permission.clone();
    changed.scope = "opaque-applied-scope".into();
    assert!(!prompt_matches(target, &friend, &changed));
    target.restorable_launch.identity_instructions = Some(initial_prompt(&friend, &changed));
    assert!(prompt_matches(target, &friend, &changed));
    target.restorable_launch.identity_instructions = None;
    target.agent_runtime_config = Some(json!({"systemPrompt":initial_prompt(&friend, &changed)}));
    assert!(prompt_matches(target, &friend, &changed));
}

#[tokio::test]
async fn each_permission_apply_creates_once_with_the_complete_new_initial_context() {
    use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let (mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut previous: Option<String> = None;
    for scope in ["opaque-apply-one", "opaque-apply-two"] {
        let permission = Permission {
            allowed: true,
            scope: scope.into(),
            allow_files: false,
            allow_memory: false,
            ..Default::default()
        };
        let expected = initial_prompt(&friend, &permission);
        let invoked = calls.clone();
        let ws = workspace.clone();
        let result = service
            .apply_neighbor_permission_with(
                &metadata,
                "owner",
                &friend,
                permission.clone(),
                service.neighbor_history_generation(),
                |_, _, cwd, grant, resume| async move {
                    invoked.fetch_add(1, Ordering::SeqCst);
                    assert!(resume.is_none());
                    assert_eq!(initial_prompt(&"a".repeat(64), &grant), expected);
                    let mut mux = service.mux.write();
                    let id = mux.create_tab(&ws).unwrap();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.cwd = Some(cwd);
                    tab.restorable_launch.identity_instructions = Some(expected);
                    Ok(id)
                },
            )
            .await
            .unwrap();
        let gateway = result["gateway_tab"].as_str().unwrap();
        assert!(prompt_matches(
            service.mux.read().get_tab(gateway).unwrap(),
            &friend,
            &permission
        ));
        if let Some(old) = previous {
            assert!(service.mux.read().get_tab(&old).is_none());
        }
        previous = Some(gateway.to_owned());
        let saved = metadata.settings().await.unwrap();
        assert_eq!(
            serde_json::to_value(saved.permissions[&friend].clone()).unwrap(),
            serde_json::to_value(permission).unwrap()
        );
        assert_eq!(saved.bindings[&friend], gateway);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
