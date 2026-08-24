use std::fs;

use super::*;
use crate::cmd::payload_source::read_from_pad_root;

fn pad() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let pad = temp.path().join("full-tab-id");
    fs::create_dir(&pad).unwrap();
    (temp, pad)
}

/// The pad file is the body. A value the test wrote must arrive in the delivered
/// params unchanged — backticks, `$`, quotes and newlines included.
#[test]
fn notify_pad_body_round_trips_byte_for_byte() {
    let (_temp, pad) = pad();
    let authored = "line one `backtick` $VAR \"quoted\"\nline two 'single'\nПривіт\n\n";
    fs::write(pad.join("message.md"), authored).unwrap();

    let body = read_from_pad_root(&pad, "message.md").expect("a pad-relative body resolves");
    let params = params_from_body("Needs a decision", "warning", None, None, Some(body));

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
    );
    assert!(params.get("body").is_none());
    assert_eq!(params["level"], serde_json::json!("verbose"));
}
