//! Provenance of notification actions: what the daemon may construct, and what
//! a caller may ask for, are two different types. These tests assert the
//! separation structurally - that the caller-facing type has nowhere to put a
//! command name - rather than that some particular command was refused.

use codeterm_types::notification::{
    adopt_caller_actions, answer_actions, CallerAction, DaemonCommand, NotificationAction,
};

use crate::api::notification_routes::CreateNotifBody;
use crate::notification::store::{DedupAdmission, NotificationProvenance, NotificationStore};

/// Tauri commands that exist and are worth stealing. None is in the daemon set,
/// and none can be named by a caller-supplied action.
const OFF_LIMITS_COMMANDS: [&str; 4] = [
    "clear_notifications",
    "mark_notification_read",
    "answer_notification",
    "respond_provider_update_prompt_",
];

fn caller_body(actions: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "title": "Deploy?", "actions": actions })
}

/// The whole claim of this slice: a caller-supplied action is structurally
/// incapable of carrying an invoke target. Not refused at a call site - the
/// type it parses into has no variant, and therefore no field, to hold one.
#[test]
fn action_provenance_caller_action_cannot_carry_an_invoke_target() {
    assert!(
        !CallerAction::KINDS.contains(&"invoke"),
        "the caller-facing kind set must not contain invoke"
    );

    for smuggled in [
        serde_json::json!({"type": "invoke", "label": "Go", "value": "clear_notifications"}),
        serde_json::json!({"type": "invoke", "label": "Go", "value": "x", "args": {"a": 1}}),
        serde_json::json!({"type": "invoke", "label": "Go"}),
        serde_json::json!({"type": "invoke", "label": "Go", "command": "respond_provider_update_prompt"}),
        serde_json::json!({"type": "url", "label": "Go", "value": "https://x", "args": {}}),
        serde_json::json!({"type": "answer", "label": "Yes", "value": "clear_notifications"}),
        serde_json::json!({"type": "dismiss", "label": "No", "args": {}}),
    ] {
        assert!(
            serde_json::from_value::<CallerAction>(smuggled.clone()).is_err(),
            "a caller action must not be able to express {smuggled}"
        );
    }

    // Exhaustive over the caller set with no wildcard arm: a variant added
    // later cannot slip past this without failing to compile.
    let every_kind = vec![
        CallerAction::Url {
            label: "Docs".into(),
            value: "https://example.invalid".into(),
        },
        CallerAction::Dismiss {
            label: "Later".into(),
        },
        CallerAction::Answer {
            label: "Rebase".into(),
        },
    ];
    for action in &every_kind {
        match action {
            CallerAction::Url { .. }
            | CallerAction::Dismiss { .. }
            | CallerAction::Answer { .. } => {}
        }
        let serialized =
            serde_json::to_value(action).expect("a caller action serializes to an object");
        let fields: Vec<&str> = serialized
            .as_object()
            .expect("an action is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            !fields.contains(&"command") && !fields.contains(&"args"),
            "no caller action carries a command name or arguments: {serialized}"
        );
    }

    // The one crossing into stored actions is total and has no invoke arm.
    for adopted in adopt_caller_actions(every_kind) {
        assert!(
            !matches!(adopted, NotificationAction::Invoke { .. }),
            "adopting a caller action must never mint an invoke action"
        );
    }
}

/// Every transport that accepts an action set parses into the caller-facing
/// type, so the refusal is the same shape everywhere and is not per-surface.
#[test]
fn action_provenance_every_caller_transport_refuses_an_invoke_action() {
    for command in OFF_LIMITS_COMMANDS {
        let hostile =
            serde_json::json!([{"type": "invoke", "label": "Go", "value": command, "args": {}}]);

        // REST: POST /api/notifications
        assert!(
            serde_json::from_value::<CreateNotifBody>(caller_body(hostile.clone())).is_err(),
            "the REST create body must refuse an invoke action naming {command}"
        );
        // Socket and CLI: notification.create
        assert!(
            serde_json::from_value::<Vec<CallerAction>>(hostile).is_err(),
            "the socket action set must refuse an invoke action naming {command}"
        );
    }

    // The refusal is the invoke, not the body: the same body without it parses.
    let legal = serde_json::json!([
        {"type": "answer", "label": "Rebase"},
        {"type": "answer", "label": "Merge"},
        {"type": "url", "label": "Docs", "value": "https://example.invalid"},
        {"type": "dismiss", "label": "Later"},
    ]);
    assert!(
        serde_json::from_value::<CreateNotifBody>(caller_body(legal.clone())).is_ok(),
        "a caller action set without an invoke must still parse"
    );
    assert_eq!(
        serde_json::from_value::<Vec<CallerAction>>(legal)
            .expect("the caller kinds still parse")
            .len(),
        4
    );
}

/// The live daemon-constructed invoke - the provider-update prompt - still
/// reaches storage intact, and serializes naming the allowlisted command.
#[test]
fn action_provenance_daemon_invoke_action_survives_construction_and_storage() {
    let mut store = NotificationStore::new(10);
    let actions = vec![NotificationAction::Invoke {
        label: "Update".into(),
        command: DaemonCommand::RespondProviderUpdatePrompt,
        args: Some(serde_json::json!({ "promptId": "prompt-1", "accept": true })),
    }];
    let admitted = store.add_provider_update(
        "codex".into(),
        "prompt-1".into(),
        "Update available".into(),
        "Install separately".into(),
        actions.clone(),
        NotificationProvenance {
            tab_id: Some("tab-1".into()),
            ..Default::default()
        },
    );
    let id = match admitted {
        DedupAdmission::Added(id) => id,
        DedupAdmission::Coalesced(id) => panic!("first provider update was coalesced into {id}"),
        DedupAdmission::Duplicate(id) => panic!("first provider update must be added, got {id}"),
    };
    let stored = store.list(false);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, id);
    assert_eq!(stored[0].actions.as_deref(), Some(actions.as_slice()));

    let wire = serde_json::to_value(stored[0]).expect("a stored row serializes");
    let action = &wire["actions"][0];
    assert_eq!(action["type"], serde_json::json!("invoke"));
    assert_eq!(
        action["command"],
        serde_json::json!(DaemonCommand::RespondProviderUpdatePrompt.as_str())
    );
    assert_eq!(action["args"]["accept"], serde_json::json!(true));
    assert!(
        action.get("value").is_none(),
        "an invoke target is not a free-form value field on the wire"
    );
}

/// The daemon side is an allowlist expressed as a type: the only command names
/// that exist are the members of the set, and nothing else reads back into it.
#[test]
fn action_provenance_daemon_command_set_is_closed() {
    assert_eq!(DaemonCommand::ALL.len(), 1);
    for command in DaemonCommand::ALL {
        assert_eq!(DaemonCommand::parse(command.as_str()), Some(command));
    }
    let distinct: std::collections::HashSet<&str> = DaemonCommand::ALL
        .into_iter()
        .map(DaemonCommand::as_str)
        .collect();
    assert_eq!(
        distinct.len(),
        DaemonCommand::ALL.len(),
        "daemon command wire values collide"
    );

    for outside in OFF_LIMITS_COMMANDS {
        assert_eq!(
            DaemonCommand::parse(outside),
            None,
            "{outside} is outside the daemon set"
        );
    }
    for rejected in [
        "",
        "   ",
        "RespondProviderUpdatePrompt",
        "respond_provider_update",
    ] {
        assert_eq!(DaemonCommand::parse(rejected), None);
    }
}

/// Negative control: the kinds a caller may still name are byte-identical to
/// what they were, in and out.
#[test]
fn action_provenance_non_invoke_kinds_are_unchanged() {
    for wire in [
        serde_json::json!({"type": "url", "label": "Docs", "value": "https://example.invalid"}),
        serde_json::json!({"type": "dismiss", "label": "Later"}),
        serde_json::json!({"type": "answer", "label": "Rebase"}),
    ] {
        let parsed: CallerAction =
            serde_json::from_value(wire.clone()).expect("a caller kind parses");
        assert_eq!(
            serde_json::to_value(&parsed).expect("a caller action serializes"),
            wire,
            "a caller kind must round-trip to the same bytes it arrived as"
        );
        let stored: NotificationAction = parsed.into();
        assert_eq!(
            serde_json::to_value(&stored).expect("a stored action serializes"),
            wire,
            "adopting a caller kind must not change a single field"
        );
    }

    let offered = answer_actions(&["Rebase".to_string(), "Merge".to_string()])
        .expect("distinct non-empty labels are legal");
    assert_eq!(
        serde_json::to_value(&offered).expect("a choice set serializes"),
        serde_json::json!([
            {"type": "answer", "label": "Rebase"},
            {"type": "answer", "label": "Merge"},
        ])
    );
    for rejected in [
        vec!["Rebase".to_string(), String::new()],
        vec!["Rebase".to_string(), " rebase ".to_string()],
    ] {
        assert!(
            answer_actions(&rejected).is_err(),
            "{rejected:?} is not a choice set a user could answer"
        );
    }

    assert!(
        adopt_caller_actions(offered)
            .iter()
            .all(|action| action.answer_label().is_some()),
        "a choice set adopts into choices and nothing else"
    );
}
