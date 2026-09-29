use std::fs;

use super::*;
use crate::cmd::payload_source::read_from_pad_root;

fn pad() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("full-tab-id");
    fs::create_dir(&pad).unwrap();
    (temp, pad)
}

fn labels(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|l| l.to_string()).collect()
}

/// The pad file is the body. A value the test wrote must arrive in the delivered
/// params unchanged — backticks, `$`, quotes and newlines included.
#[test]
fn notify_pad_body_round_trips_byte_for_byte() {
    let (_temp, pad) = pad();
    let authored = "line one `backtick` $VAR \"quoted\"\nline two 'single'\nПривіт\n\n";
    fs::write(pad.join("message.md"), authored).unwrap();

    let body = read_from_pad_root(&pad, "message.md").expect("a pad-relative body resolves");
    let params = params_from_body(
        "Needs a decision",
        "warning",
        None,
        None,
        Some(body),
        &[],
        Offer::default(),
        None,
    )
    .expect("a body with no choices is legal");

    assert_eq!(params["body"], serde_json::json!(authored));
    assert_eq!(params["title"], serde_json::json!("Needs a decision"));
    assert_eq!(params["severity"], serde_json::json!("warning"));
}

#[test]
fn notify_pad_body_rejects_paths_escaping_the_pad() {
    let (temp, pad) = pad();
    fs::write(temp.path().join("outside.md"), "not yours").unwrap();
    fs::write(pad.join("inside.md"), "yours").unwrap();

    assert!(
        read_from_pad_root(&pad, "../outside.md").is_err(),
        "traversal out of the pad must be rejected"
    );
    let absolute = temp.path().join("outside.md");
    assert!(
        read_from_pad_root(&pad, absolute.to_str().unwrap()).is_err(),
        "an absolute path must be rejected"
    );
    assert_eq!(read_from_pad_root(&pad, "inside.md").unwrap(), "yours");
}

/// No pad path means no body key at all — a title-only notification stays legal.
#[test]
fn notify_pad_body_absent_leaves_no_body_key() {
    let params = params_from_body(
        "Build complete",
        "info",
        Some(NotifyLevel::Verbose),
        None,
        None,
        &[],
        Offer::default(),
        None,
    )
    .expect("a title-only notification is legal");
    assert!(params.get("body").is_none());
    assert_eq!(params["level"], serde_json::json!("verbose"));
}

/// `--choice` labels reach the daemon as answer actions and nothing else: the
/// delivered params carry the labels the caller typed, in order, with no command
/// name and no arguments anywhere in the set.
#[test]
fn notify_answer_choice_labels_round_trip_into_the_actions() {
    let params = params_from_body(
        "Which branch?",
        "info",
        Some(NotifyLevel::ActionNeeded),
        None,
        None,
        &labels(&["Rebase", "Merge", "Neither"]),
        Offer::default(),
        None,
    )
    .expect("distinct non-empty labels are a legal choice set");

    let actions = params["actions"]
        .as_array()
        .expect("choices are delivered as an action array");
    assert_eq!(
        actions
            .iter()
            .map(|action| action["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Rebase", "Merge", "Neither"]
    );
    for action in actions {
        assert_eq!(action["type"], serde_json::json!("answer"));
        // Sorted, because serde_json's key order is the map's, not the insertion
        // order — the claim is which fields exist, never the order they serialise in.
        let mut fields: Vec<&str> = action
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            vec!["label", "type"],
            "a choice carries its label and nothing that could name a command"
        );
    }
}

/// Naming no choice leaves the key off entirely, so an ordinary notification is
/// byte-for-byte what it was before choices existed.
#[test]
fn notify_answer_no_choice_leaves_no_actions_key() {
    let params = params_from_body(
        "Build complete",
        "info",
        None,
        None,
        None,
        &[],
        Offer::default(),
        None,
    )
    .expect("a notification without choices is legal");
    assert!(params.get("actions").is_none());
    assert_eq!(choice_actions(&[]).unwrap(), None);
}

/// A blank label names nothing and two equal labels name the same thing twice:
/// both are refused before anything is sent.
#[test]
fn notify_answer_choice_refuses_blank_and_duplicate_labels() {
    for rejected in [
        labels(&["Rebase", ""]),
        labels(&["   "]),
        labels(&["Rebase", "Rebase"]),
        labels(&["Rebase", " rebase "]),
    ] {
        assert!(
            params_from_body(
                "Which branch?",
                "info",
                None,
                None,
                None,
                &rejected,
                Offer::default(),
                None,
            )
            .is_err(),
            "{rejected:?} must be refused before it is sent"
        );
        assert!(choice_actions(&rejected).is_err());
    }
}

/// Choices are labels only. The flag has no free-text sibling, and a label is
/// carried verbatim rather than reinterpreted as anything the shell could mean.
#[test]
fn notify_answer_choice_labels_are_carried_verbatim() {
    let awkward = labels(&["Yes, $NOW", "No `never`"]);
    let params = params_from_body(
        "Proceed?",
        "info",
        None,
        None,
        None,
        &awkward,
        Offer::default(),
        None,
    )
    .expect("punctuation is not a reason to refuse a label");
    let carried: Vec<String> = params["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| action["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(carried, awkward);
}

/// A question's answer travels back to the tab that asked, and the daemon records
/// that tab from what the caller sent. Without it the buttons render and the tap
/// has nowhere to go — the chat card never appears at all, because there is no
/// tab to draw it in. Every other tab-scoped CLI command already attaches this.
#[test]
fn notify_answer_choices_carry_the_asking_tab() {
    let params = params_from_body(
        "Which branch?",
        "info",
        Some(NotifyLevel::ActionNeeded),
        None,
        None,
        &labels(&["Rebase", "Merge"]),
        Offer::default(),
        Some("tab-7"),
    )
    .expect("a legal choice set");
    assert_eq!(params["caller_tab_id"], serde_json::json!("tab-7"));
}

/// A plain send still names its asking tab: the daemon resolves provenance and team
/// scope from it, so a send without choices must not arrive anonymous.
#[test]
fn notification_deep_link_keeps_the_asking_tab() {
    for choices in [Vec::new(), labels(&["Rebase", "Merge"])] {
        let params = params_from_body(
            "Build complete",
            "info",
            Some(NotifyLevel::ActionNeeded),
            None,
            None,
            &choices,
            Offer::default(),
            Some("tab-7"),
        )
        .expect("a legal notification");
        let params = with_target(params, Some("tab-target"), Some("m-1"));
        assert_eq!(params["caller_tab_id"], serde_json::json!("tab-7"));
        assert_eq!(params["target_tab_id"], serde_json::json!("tab-target"));
    }
    let untargeted = params_from_body(
        "Build complete",
        "info",
        None,
        None,
        None,
        &[],
        Offer::default(),
        Some("tab-7"),
    )
    .expect("a title-only notification is legal");
    assert_eq!(untargeted["caller_tab_id"], serde_json::json!("tab-7"));
}

/// `notify_unseen` — the daemon's report reaching the caller, and staying away
/// on the ordinary case.
mod notify_unseen {
    use super::super::{handle_send, unseen_notice, without_unseen};
    use crate::client::{RpcClient, SocketCallError};

    fn reported() -> serde_json::Value {
        serde_json::json!({
            "id": "notice-1",
            "unseen": {
                "closed": [
                    { "surface": "bell", "reason": "the bell indicator is hidden" },
                    { "surface": "desktop", "reason": "desktop notifications are off" },
                    { "surface": "push", "reason": "no push channel is routed" },
                ]
            }
        })
    }

    struct Recorder {
        answer: serde_json::Value,
        calls: Vec<String>,
    }

    impl RpcClient for Recorder {
        fn call_detailed(
            &mut self,
            method: &str,
            _params: Option<serde_json::Value>,
        ) -> Result<serde_json::Value, SocketCallError> {
            self.calls.push(method.to_string());
            Ok(self.answer.clone())
        }
    }

    #[test]
    fn notify_unseen_is_absent_on_the_ordinary_result() {
        let ordinary = serde_json::json!({ "id": "notice-1" });
        assert!(
            unseen_notice(&ordinary).is_none(),
            "a notification something will show says nothing extra"
        );
    }

    #[test]
    fn notify_unseen_reaches_the_caller_with_a_reason_per_surface() {
        let notice = unseen_notice(&reported()).expect("the report travels to the caller");
        for surface in ["bell", "desktop", "push"] {
            assert!(
                notice.contains(surface),
                "the notice names the {surface} surface, got: {notice}"
            );
        }
        assert!(
            notice.contains("the bell indicator is hidden"),
            "the notice carries the daemon's own reason rather than restating one: {notice}"
        );
    }

    /// The id is what the caller came for and it survives the report — the
    /// notification was accepted, so nothing about this reads as a failure.
    #[test]
    fn notify_unseen_never_displaces_the_id() {
        let trimmed = without_unseen(&reported());
        assert_eq!(trimmed["id"], serde_json::json!("notice-1"));
        assert!(
            trimmed.get("unseen").is_none(),
            "the human rendering of the result stays the id it has always been"
        );
    }

    /// Both shapes go down the same accepted path: one call, no early exit,
    /// nothing that turns an accepted record into an error.
    #[test]
    fn notify_unseen_leaves_the_send_path_successful() {
        for answer in [serde_json::json!({ "id": "notice-1" }), reported()] {
            let mut client = Recorder {
                answer,
                calls: Vec::new(),
            };
            handle_send(&mut client, false, serde_json::json!({ "title": "done" }));
            assert_eq!(client.calls, vec!["notification.create".to_string()]);
        }
    }
}

mod notification_deep_link {
    use super::super::with_target;

    #[test]
    fn notification_deep_link_params_carry_the_named_ids() {
        let params = with_target(
            serde_json::json!({ "title": "Approve?" }),
            Some("tab-target"),
            Some("needs-you-message-1"),
        );
        assert_eq!(params["target_tab_id"], "tab-target");
        assert_eq!(params["chat_message_id"], "needs-you-message-1");
        assert_eq!(
            params["title"], "Approve?",
            "the rest of the request is untouched"
        );
    }

    #[test]
    fn notification_deep_link_params_stay_legacy_shaped_without_a_target() {
        let params = with_target(serde_json::json!({ "title": "Approve?" }), None, None);
        assert!(params.get("target_tab_id").is_none());
        assert!(params.get("chat_message_id").is_none());
    }
}

#[test]
fn notify_send_validates_title_and_target() {
    let limit = codeterm_types::notification::MAX_CALLER_TITLE_CHARS;
    let params = |title: &str| {
        params_from_body(title, "info", None, None, None, &[], Offer::default(), None)
    };

    assert!(params("").is_err(), "an empty title is refused");
    assert!(params("   ").is_err(), "a blank title is refused");
    assert!(
        params(&"t".repeat(limit + 1)).is_err(),
        "an unbounded title is refused"
    );
    let bounded = "t".repeat(limit);
    assert_eq!(
        params(&bounded).unwrap()["title"],
        serde_json::json!(bounded)
    );
    assert_eq!(
        params("  opaque  ").unwrap()["title"],
        serde_json::json!("opaque")
    );

    assert!(
        vet_target_tab(Some("  ")).is_err(),
        "a blank target names no tab"
    );
    assert!(vet_target_tab(Some("opaque-tab")).is_ok());
    assert!(vet_target_tab(None).is_ok());
}
