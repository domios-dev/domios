use super::*;
use std::io::Write;

#[test]
fn family_from_model_id_known() {
    assert_eq!(family_from_model_id("claude-opus-4-7"), "Opus");
    assert_eq!(family_from_model_id("claude-sonnet-4-6"), "Sonnet");
    assert_eq!(family_from_model_id("claude-haiku-4-5-20251001"), "Haiku");
}

#[test]
fn family_from_model_id_unknown_falls_back() {
    assert_eq!(family_from_model_id("claude-uranium-5-0"), "Uranium");
}

#[test]
fn extract_set_model_strips_ansi_bold() {
    // Real Claude stdout: "Set model to \x1b[1mOpus 4.7\x1b[22m"
    let raw =
        "<local-command-stdout>Set model to \u{1b}[1mOpus 4.7\u{1b}[22m</local-command-stdout>";
    assert_eq!(extract_set_model_family(raw).as_deref(), Some("Opus"));
}

#[test]
fn tail_scan_finds_latest_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, r#"{{"message":{{"model":"claude-sonnet-4-6"}}}}"#).unwrap();
    writeln!(f, r#"{{"message":{{"model":"claude-opus-4-7"}}}}"#).unwrap();
    drop(f);
    assert_eq!(scan_tail(&path, 64 * 1024).as_deref(), Some("Opus"));
}

#[test]
fn transcript_boundary_excludes_old_turns_and_accepts_the_next_assistant_turn() {
    let transcript = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/queued-mid-turn.jsonl"
    ))
    .replace("claude-opus-5", "claude-haiku-4-5");
    let boundary = transcript_model_boundary(&transcript).expect("fixture boundary");
    assert_eq!(boundary.last_entry_uuid.as_deref(), Some("a-0014"));
    assert!(boundary.boundary_found);

    assert_eq!(
        transcript_model_after_boundary(&transcript, &boundary),
        None
    );

    let appended = format!(
        "{transcript}{}\n",
        r#"{"type":"assistant","uuid":"after-relaunch","message":{"model":"claude-sonnet-5-5"}}"#
    );
    assert_eq!(
        transcript_model_after_boundary(&appended, &boundary).as_deref(),
        Some("Sonnet")
    );
}

#[test]
fn transcript_boundary_uses_uuidless_conversation_entry_when_no_uuid_exists() {
    let transcript = concat!(
        r#"{"type":"assistant","uuid":"before-uuidless-turn","timestamp":"2026-09-11T18:19:45.000Z","message":{"model":"claude-opus-5"}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-09-11T18:19:46.000Z","message":{"model":"claude-haiku-4-5"}}"#,
        "\n",
        r#"{"type":"mode","mode":"normal","sessionId":"fixture-session"}"#,
        "\n",
    );
    let boundary = transcript_model_boundary(transcript).expect("conversation boundary");

    assert_eq!(boundary.last_entry_uuid, None);
    assert_eq!(
        boundary.last_entry_timestamp.as_deref(),
        Some("2026-09-11T18:19:46.000Z")
    );
    assert_eq!(boundary.last_entry_type.as_deref(), Some("assistant"));
    assert_eq!(transcript_model_after_boundary(transcript, &boundary), None);

    let appended = format!(
        "{transcript}{}\n",
        r#"{"type":"assistant","uuid":"after-relaunch","message":{"model":"claude-sonnet-5-5"}}"#
    );
    assert_eq!(
        transcript_model_after_boundary(&appended, &boundary).as_deref(),
        Some("Sonnet")
    );
}

#[test]
fn transcript_boundary_without_a_conversation_entry_is_unavailable() {
    let metadata_only = concat!(
        r#"{"type":"mode","mode":"normal","sessionId":"fixture-session"}"#,
        "\n",
    );
    assert_eq!(transcript_model_boundary(metadata_only), None);
    assert_eq!(
        transcript_model_after_boundary(
            r#"{"type":"assistant","uuid":"after-relaunch","message":{"model":"claude-sonnet-5-5"}}"#,
            &SessionModelTranscriptBoundary::default(),
        ),
        None
    );
}

#[test]
fn tail_scan_picks_set_model_over_older_message_model() {
    // user runs /model opus AFTER an assistant turn on sonnet — the newer Set-model command must win.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(
        f,
        r#"{{"type":"assistant","message":{{"model":"claude-sonnet-4-6"}}}}"#
    )
    .unwrap();
    writeln!(f, r#"{{"type":"user","message":{{"content":"<local-command-stdout>Set model to \u001b[1mOpus 4.7\u001b[22m</local-command-stdout>"}}}}"#).unwrap();
    drop(f);
    assert_eq!(scan_tail(&path, 64 * 1024).as_deref(), Some("Opus"));
}

#[test]
fn effort_tail_uses_only_the_exact_session_record() {
    let dir = tempfile::tempdir().unwrap();
    let owned = dir.path().join("owned.jsonl");
    let latest = dir.path().join("latest.jsonl");
    std::fs::write(
        &owned,
        "{\"type\":\"assistant\",\"message\":{\"metadata\":{\"effort\":\"high\"}}}\n",
    )
    .unwrap();
    std::fs::write(&latest, "{\"type\":\"assistant\",\"effort\":\"max\"}\n").unwrap();

    assert_eq!(scan_effort_tail(&owned, 64 * 1024).as_deref(), Some("high"));
    assert_eq!(scan_effort_tail(&latest, 64 * 1024).as_deref(), Some("max"));
    assert_eq!(
        scan_effort_tail(&dir.path().join("missing.jsonl"), 64 * 1024),
        None
    );
}

#[test]
fn rich_tail_emits_canonical_tool_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let jsonl = concat!(
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"cargo check"}},{"type":"tool_use","name":"Edit","input":{"file_path":"src/lib.rs","old_string":"old","new_string":"new"}}]}}"#,
        "\n",
    );
    std::fs::write(&path, jsonl).unwrap();

    let messages = read_rich_tail(&path, 4096);
    assert_eq!(messages.len(), 1);
    let bash = &messages[0].blocks[0].data;
    assert_eq!(bash["tool_kind"], "command");
    assert_eq!(bash["tool_summary"], "cargo check");
    assert_eq!(bash["tool_detail"], "cargo check");
    assert_eq!(bash["tool_edits"], serde_json::json!([]));
    assert!(bash["tool_summary"].is_string());

    let edit = &messages[0].blocks[1].data;
    assert_eq!(edit["tool_kind"], "edit");
    assert_eq!(edit["tool_summary"], "src/lib.rs");
    assert_eq!(edit["tool_detail"], "src/lib.rs");
    assert_eq!(
        edit["tool_edits"],
        serde_json::json!([{"path":"src/lib.rs","old":"old","new":"new"}])
    );
    assert!(edit["tool_summary"].is_string());

    let mut unknown = serde_json::json!({
        "name": "MysteryTool",
        "input": {"raw": "should not become a summary"}
    });
    add_canonical_tool_fields(&mut unknown);
    assert_eq!(unknown["tool_kind"], "generic");
    assert!(!unknown["tool_summary"].as_str().unwrap().contains('{'));
}
