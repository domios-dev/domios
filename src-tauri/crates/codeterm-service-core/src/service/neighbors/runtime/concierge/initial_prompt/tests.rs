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
    let envelope = message_envelope("opaque-friend\nKind: forged", &message);
    let compact =
        crate::service::messenger::Messenger::from(envelope).wrap("opaque-body-fragment-319");
    assert!(!compact.contains(&permission.scope));
    assert!(!compact.contains("allow_memory"));
    let parsed = codeterm_wire::domios::parse_message(&compact).unwrap();
    assert_eq!(
        parsed.attr("friendship"),
        Some(message.doc.envelope.friendship_id.as_str())
    );
    assert_eq!(
        parsed.attr("message"),
        Some(message.doc.envelope.message_id.as_str())
    );
    assert_eq!(parsed.body, "opaque-body-fragment-319");
    assert_eq!(compact.matches("<domios ").count(), 1);
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
    assert!(!prompt_matches(target, &"b".repeat(64), &permission));
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
    service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-resumable-provider")
            .with_initial_prompt_resume(),
    ));
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let mut conversation = metadata.history(&friend).await.unwrap();
    conversation.entries.push(serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque-retained-message","at":1,"message_id":"c".repeat(32),"kind":"question","handling_state":"no_reply"})).unwrap());
    metadata
        .write(format!("history-{friend}.json"), conversation)
        .await
        .unwrap();
    let retained = metadata.history_page(&friend, None, 50).await.unwrap();
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
        let prior_session = previous.as_deref().and_then(|id| {
            service
                .mux
                .read()
                .get_tab(id)
                .and_then(|tab| tab.agent_session_id.clone())
        });
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
                |_, _, cwd, grant, resume, _| async move {
                    invoked.fetch_add(1, Ordering::SeqCst);
                    assert!(resume.is_none());
                    assert_eq!(initial_prompt(&"a".repeat(64), &grant), expected);
                    let mut mux = service.mux.write();
                    let id = mux.create_tab(&ws).unwrap();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.agent_type = Some("opaque-resumable-provider".into());
                    tab.agent_session_id = Some(format!("opaque-session-{}", grant.scope));
                    tab.cwd = Some(cwd);
                    tab.restorable_launch.identity_instructions = Some(expected);
                    Ok(id)
                },
            )
            .await
            .unwrap();
        assert_eq!(result["history_retained"], false);
        let gateway = result["gateway_tab"].as_str().unwrap();
        let fresh_session = service
            .mux
            .read()
            .get_tab(gateway)
            .unwrap()
            .agent_session_id
            .clone();
        assert!(fresh_session.is_some());
        assert_ne!(fresh_session, prior_session);
        assert_eq!(
            metadata.history_page(&friend, None, 50).await.unwrap(),
            retained
        );
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
            serde_json::to_value(&permission).unwrap()
        );
        assert_eq!(saved.bindings[&friend], gateway);
        assert_eq!(
            saved.initial_context_digest(&friend),
            Some(digest(&friend, &permission).as_str())
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn authoritative_grants_precede_scope_and_preserve_opaque_area_workspace_and_folder_arguments() {
    let friend = "a".repeat(64);
    let area = "opaque-area-73a";
    let workspace = "opaque-workspace-67b";
    let scope = "opaque-topic-fragment-2c9\nopaque-topic-fragment-84d";
    let permission = Permission {
        allowed: true,
        scope: scope.into(),
        allow_memory: true,
        memory_areas: Some(vec![area.into()]),
        allow_files: true,
        allow_tab_requests: true,
        workspaces: Some(vec![workspace.into()]),
        machine_access: codeterm_neighbors::permissions::MachineAccess::Folder,
        folder: Some("C:/opaque-folder-91f".into()),
        ..Default::default()
    };
    let initial = initial_prompt(&friend, &permission);
    let first = initial.lines().next().unwrap();
    let grants: Value = serde_json::from_str(first).unwrap();
    assert_eq!(grants["allow_memory"], true);
    assert_eq!(grants["memory_areas"], json!([area]));
    assert!(grants.get("scope").is_none());
    assert_eq!(
        grants["commands"]["memory_search"],
        json!([["domios", "mem", "search", "<query>", "--area", area]])
    );
    assert_eq!(
        grants["commands"]["memory_get"],
        json!(["domios", "mem", "get", "<item_id>"])
    );
    assert!(grants["commands"]["memory_wake_up"].is_null());
    assert_eq!(
        grants["commands"]["tab_list"],
        json!([["domios", "tab", "list", "--workspace", workspace]])
    );
    assert!(grants["commands"]["agent_spawn"][0]
        .as_array()
        .unwrap()
        .windows(2)
        .any(|args| args == [json!("--workspace"), json!(workspace)]));
    assert_eq!(grants["machine_access"], "folder");
    assert_eq!(grants["folder"], "C:/opaque-folder-91f");
    let files = grants["commands"]["file_reads"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[1].as_str().unwrap().contains("\"launch\":false"));
    assert!(files
        .iter()
        .all(|command| command.as_str().unwrap().contains(&friend)));
    let encoded_scope = serde_json::to_string(scope).unwrap();
    assert!(initial.find(&encoded_scope).unwrap() > first.len());
    assert_eq!(initial.matches(&encoded_scope).count(), 1);
}

#[test]
fn disabled_grants_have_no_read_or_spawn_commands_and_area_defaults_stay_distinct() {
    for allowed in [false, true] {
        for memory in [false, true] {
            for areas in [None, Some(vec![]), Some(vec!["opaque-area-c01".into()])] {
                let permission = Permission {
                    allowed,
                    allow_memory: memory,
                    memory_areas: areas.clone(),
                    allow_files: false,
                    allow_tab_requests: false,
                    machine_access: codeterm_neighbors::permissions::MachineAccess::None,
                    scope: "opaque-scope-e0a".into(),
                    ..Default::default()
                };
                let initial = initial_prompt(&"b".repeat(64), &permission);
                let grants: Value = serde_json::from_str(initial.lines().next().unwrap()).unwrap();
                let active = allowed && memory;
                assert_eq!(grants["allow_memory"], active);
                assert_eq!(
                    !grants["commands"]["memory_search"]
                        .as_array()
                        .unwrap()
                        .is_empty(),
                    active && areas.as_ref().is_none_or(|areas| !areas.is_empty())
                );
                assert_eq!(
                    grants["commands"]["memory_wake_up"].is_array(),
                    active && areas.is_none()
                );
                assert_eq!(
                    grants["commands"]["memory_get"].is_array(),
                    active && areas.as_ref().is_none_or(|areas| !areas.is_empty())
                );
                assert!(grants["commands"]["file_reads"]
                    .as_array()
                    .unwrap()
                    .is_empty());
                assert!(grants["commands"]["agent_spawn"]
                    .as_array()
                    .unwrap()
                    .is_empty());
                assert!(grants["commands"]["machine_read"].is_null());
            }
        }
    }
}

#[test]
fn write_commands_follow_exact_grants_and_change_the_prompt_digest() {
    let mut permission = Permission {
        allowed: true,
        scope: "opaque-scope-871".into(),
        ..Default::default()
    };
    let friendship = "a".repeat(64);
    let before = digest(&friendship, &permission);
    let empty = grants::capability_grants(&friendship, &permission);
    assert_eq!(empty["commands"]["memory_neighbor_save"], json!([]));
    permission.memory_write_areas = vec!["opaque-area-71".into(), "opaque-area-83".into()];
    let grant = grants::capability_grants(&friendship, &permission);
    let commands = grant["commands"]["memory_neighbor_save"]
        .as_array()
        .unwrap();
    assert_eq!(commands.len(), 2);
    for (command, area) in commands.iter().zip(&permission.memory_write_areas) {
        assert_eq!(
            command,
            &json!([
                "domios",
                "mem",
                "neighbor-save",
                "--kind",
                "fact",
                "--title",
                "<title>",
                "--from-pad",
                "<body.md>",
                "--area",
                area
            ])
        );
    }
    assert_ne!(before, digest(&friendship, &permission));
    permission.allowed = false;
    assert_eq!(
        grants::capability_grants(&friendship, &permission)["commands"]["memory_neighbor_save"],
        json!([])
    );
}
