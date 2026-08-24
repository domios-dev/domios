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
    let params = params_from_body("Needs a decision", "warning", None, None, Some(body), &[])
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
    let params = params_from_body("Build complete", "info", None, None, None, &[])
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
            params_from_body("Which branch?", "info", None, None, None, &rejected).is_err(),
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
    let params = params_from_body("Proceed?", "info", None, None, None, &awkward)
        .expect("punctuation is not a reason to refuse a label");
    let carried: Vec<String> = params["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| action["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(carried, awkward);
}
