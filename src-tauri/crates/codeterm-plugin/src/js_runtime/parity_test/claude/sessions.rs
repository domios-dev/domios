//! Claude parity: tab-state classification, interactive-event extraction,
//! and JSONL session delta/flatten parsing.

use super::super::*;
use super::*;

/// AS3 (Case B): the real claude plugin.js classifies a rate-limit menu as
/// `RateLimited` through the QuickJS runtime, and its wire output round-trips
/// into the native `TabStateClassification` types — confirm-wait keys + a
/// scheduled `continue your work` resume. This is the native↔JS parity for
/// the claude classifier: the JS verdict parses natively into the exact
/// generic shape the core reactor acts on.
#[test]
fn classify_rate_limit_menu_round_trips_native_types() {
    use crate::tab_state::{TabState, TabStateSnapshot};
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    let snap = TabStateSnapshot {
        tab_id: "tab-1".into(),
        agent_type: "claude".into(),
        session_id: None,
        cwd: None,
        screen_text: concat!(
            "You've hit your session limit \u{b7} resets 4:20pm (Europe/Riga)\n",
            "/rate-limit-options\n\n",
            "What do you want to do?\n\n",
            "> 1. Stop and wait for limit to reset\n",
            "  2. Upgrade your plan\n",
            "  3. Upgrade to Team plan\n\n",
            "Enter to confirm \u{b7} Esc to cancel\n",
        )
        .into(),
    };
    let c = j
        .classify_tab_state(&snap)
        .expect("claude classifier must return a verdict");
    assert_eq!(c.state, TabState::RateLimited, "state");
    assert!(
        c.confidence >= 0.8_f32,
        "confidence {} below threshold",
        c.confidence
    );
    let action = c.action.expect("rate-limit verdict carries an action");
    // Default highlight is the wait option → a bare Enter confirms it.
    assert_eq!(
        action.keys.as_deref(),
        Some(["enter".to_string()].as_slice())
    );
    let resume = action
        .delayed_resume
        .expect("a resume is scheduled when the reset time parsed");
    assert_eq!(resume.text, "continue your work");
    assert_eq!(
        resume.then_keys.as_deref(),
        Some(["enter".to_string()].as_slice())
    );
    // hand straight to delayed-send (parse_iso_to_ms there).
    let reset_at = c.data.and_then(|d| d.reset_at).expect("data.resetAt set");
    let reset = chrono::DateTime::parse_from_rfc3339(&reset_at).expect("resetAt is RFC3339");
    let resume_at = chrono::DateTime::parse_from_rfc3339(&resume.at).expect("resume.at is RFC3339");
    assert!(
        resume_at > reset,
        "resume must fire after reset (buffer): {} vs {}",
        resume.at,
        reset_at
    );
}

/// The no-false-positive wall (mirrors the negative corpus): a normal
/// streaming screen yields no actionable verdict — never `RateLimited`, never
/// a key action — so the reactor stays its hand.
#[test]
fn classify_normal_screen_is_not_actionable() {
    use crate::tab_state::{TabState, TabStateSnapshot};
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    let snap = TabStateSnapshot {
        tab_id: "tab-1".into(),
        agent_type: "claude".into(),
        session_id: None,
        cwd: None,
        screen_text:
            "Reading src/router.ts and tracing the middleware chain.\n(esc to interrupt)\n".into(),
    };
    if let Some(c) = j.classify_tab_state(&snap) {
        assert_ne!(c.state, TabState::RateLimited);
        assert_ne!(c.state, TabState::ClarifyingQuestion);
        assert_ne!(c.state, TabState::WaitingOnAuth);
        assert!(
            c.action.and_then(|a| a.keys).is_none(),
            "normal screen must not yield a key action"
        );
    }
}

#[test]
fn parse_interactive_events_native_js_parity() {
    // runs — the parity gate must not silently skip.
    let n = ClaudeProvider::new();
    let j = claude_interactive_js();
    let fixture = ask_user_question_fixture();
    let native = n.parse_interactive_events(&fixture);
    let js = j.parse_interactive_events(&fixture);
    assert_eq!(native.len(), 2, "native must extract two questions");
    assert_eq!(
        native, js,
        "native and JS parseInteractiveEvents must agree on the same fixture"
    );
}

#[test]
fn parse_session_delta_handles_polymorphic_content() {
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");

    // Mixed JSONL: summary (drop), user text (1 block), assistant with text + thinking + tool_use (3 blocks), user tool_result (1 block), and a malformed line (drop).
    let jsonl = concat!(
        r#"{"type":"summary","x":1}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-05-20T10:00:00Z"}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"},{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"u1","name":"Bash","input":{"command":"ls"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u1","content":"foo bar"}]}}"#,
        "\n",
        "not-json-at-all\n",
    );

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    assert_eq!(
        delta.new_offset,
        jsonl.len() as u64,
        "should consume the entire file (last line ends in \\n)"
    );
    assert_eq!(delta.messages.len(), 3, "3 user/assistant entries");

    let m0 = &delta.messages[0];
    assert_eq!(m0.role, "user");
    assert_eq!(m0.blocks.len(), 1);
    assert_eq!(m0.blocks[0].kind, "text");

    let m1 = &delta.messages[1];
    assert_eq!(m1.role, "assistant");
    assert_eq!(m1.blocks.len(), 3);
    let kinds: Vec<&str> = m1.blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(kinds, vec!["text", "thinking", "tool_use"]);
    assert_eq!(
        m1.blocks[2].data.get("name").and_then(|v| v.as_str()),
        Some("Bash"),
    );

    let m2 = &delta.messages[2];
    assert_eq!(m2.blocks[0].kind, "tool_result");
    assert_eq!(
        m2.blocks[0]
            .data
            .get("tool_use_id")
            .and_then(|v| v.as_str()),
        Some("u1"),
    );
}

#[test]
fn model_switch_transcript_boundary_matches_native_and_js() {
    if require_plugins().is_none() {
        return;
    }
    use crate::provider::AgentProvider;

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let evidence = temp.path().join("evidence");
    // Windows resolves the home directory without HOME, so both paths are rooted explicitly.
    let _env = EnvGuard::new(&[(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        evidence.to_str().unwrap(),
    )]);
    let cwd = cwd.to_string_lossy().into_owned();
    let session_id = uuid::Uuid::new_v4().to_string();
    let native = ClaudeProvider::new();
    let js = super::super::find_session::claude_js_rooted_at(&temp.path().join("projects"));
    let path = js
        .session_file_path(&cwd, &session_id)
        .expect("Claude transcript path");
    assert!(path.starts_with(temp.path()), "{}", path.display());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let transcript = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-claude/tests/fixtures/queued-mid-turn.jsonl"
    ))
    .replace("claude-opus-5", "claude-haiku-4-5");
    std::fs::write(&path, &transcript).unwrap();
    assert!(crate::session_evidence::record(
        &session_id,
        path.to_str().unwrap()
    ));
    assert_eq!(
        native.session_file_path(&cwd, &session_id),
        Some(path.clone())
    );

    let boundary = native
        .session_model_transcript_boundary(&cwd, &session_id)
        .expect("native captures the relaunch boundary");
    assert_eq!(boundary.last_entry_uuid.as_deref(), Some("a-0014"));
    assert!(boundary.boundary_found);
    assert_eq!(
        js.session_model_transcript_boundary(&cwd, &session_id),
        Some(boundary.clone())
    );
    assert_eq!(
        native.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        None
    );
    assert_eq!(
        js.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        None
    );

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","uuid":"after-relaunch","message":{{"model":"claude-sonnet-5-5"}}}}"#
    )
    .unwrap();
    let native_model = native
        .detect_session_model_after_boundary(&cwd, &session_id, &boundary)
        .expect("native observes the appended assistant turn");
    assert_eq!(
        js.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        Some(native_model)
    );
}

#[test]
fn model_switch_uuidless_conversation_boundary_matches_native_and_js() {
    if require_plugins().is_none() {
        return;
    }
    use crate::provider::AgentProvider;

    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let evidence = temp.path().join("evidence");
    let _env = EnvGuard::new(&[(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        evidence.to_str().unwrap(),
    )]);
    let cwd = cwd.to_string_lossy().into_owned();
    let session_id = uuid::Uuid::new_v4().to_string();
    let native = ClaudeProvider::new();
    let js = super::super::find_session::claude_js_rooted_at(&temp.path().join("projects"));
    let path = js
        .session_file_path(&cwd, &session_id)
        .expect("Claude transcript path");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let transcript = concat!(
        r#"{"type":"assistant","uuid":"before-uuidless-turn","timestamp":"2026-09-11T18:19:45.000Z","message":{"model":"claude-opus-5"}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-09-11T18:19:46.000Z","message":{"model":"claude-haiku-4-5"}}"#,
        "\n",
        r#"{"type":"mode","mode":"normal","sessionId":"fixture-session"}"#,
        "\n",
    );
    std::fs::write(&path, transcript).unwrap();
    assert!(crate::session_evidence::record(
        &session_id,
        path.to_str().unwrap()
    ));

    let boundary = native
        .session_model_transcript_boundary(&cwd, &session_id)
        .expect("native captures a UUID-less conversation boundary");
    assert_eq!(
        js.session_model_transcript_boundary(&cwd, &session_id),
        Some(boundary.clone())
    );
    assert_eq!(boundary.last_entry_uuid, None);
    assert_eq!(boundary.last_entry_type.as_deref(), Some("assistant"));
    assert_eq!(
        native.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        None
    );
    assert_eq!(
        js.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        None
    );
    let unavailable = codeterm_types::agent::SessionModelTranscriptBoundary::default();
    assert_eq!(
        native.detect_session_model_after_boundary(&cwd, &session_id, &unavailable),
        None
    );
    assert_eq!(
        js.detect_session_model_after_boundary(&cwd, &session_id, &unavailable),
        None
    );

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","uuid":"after-relaunch","message":{{"model":"claude-sonnet-5-5"}}}}"#
    )
    .unwrap();
    assert_eq!(
        native.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        Some("Sonnet".into())
    );
    assert_eq!(
        js.detect_session_model_after_boundary(&cwd, &session_id, &boundary),
        Some("Sonnet".into())
    );

    std::fs::write(
        &path,
        r#"{"type":"mode","mode":"normal","sessionId":"fixture-session"}"#,
    )
    .unwrap();
    assert_eq!(
        native.session_model_transcript_boundary(&cwd, &session_id),
        None
    );
    assert_eq!(
        js.session_model_transcript_boundary(&cwd, &session_id),
        None
    );
}

/// Eval: the host flatten path (real plugin.js `parseSessionDelta` →
/// Rust `rich_to_chat_msgs`) must produce the SAME normalized `ChatMsg`
/// shape the desktop TS parser (`parseJsonlSession`) produces, so the
/// desktop can switch to the host path and the TS duplicate can be
/// deleted. This is the bench guarding that consolidation.
#[test]
fn rich_flatten_parity_block_derivable_kinds() {
    if require_plugins().is_none() {
        return;
    }
    use crate::rich_to_chat::rich_to_chat_msgs;
    let j = load_js("claude");

    // Covers every block kind plugin.js already passes through:
    let jsonl = concat!(
        r#"{"type":"user","message":{"role":"user","content":"refactor please"},"timestamp":"2026-05-20T10:00:00Z"}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"on it"},{"type":"thinking","thinking":"let me think a lot AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA done"},{"type":"tool_use","id":"u1","name":"Bash","input":{"command":"ls -la /very/long/path/that/exceeds/eighty/characters/for/sure/abcdefghijklmnop"}}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_result","tool_use_id":"u1","content":"file listing"}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"u2","content":"grep output"}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"see image"},{"type":"image","source":{"media_type":"image/png","data":"BASE64DATA"}}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"image","source":{"media_type":"image/jpeg","data":"IMGBYTES"}}]}}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    let msgs: Vec<_> = delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
        .collect();

    let by_kind = |k: &str| msgs.iter().filter(|m| m.kind == k).collect::<Vec<_>>();

    let users = by_kind("user");
    assert!(
        users.iter().any(|m| m.content == "refactor please"),
        "user text lost: {users:#?}"
    );

    assert!(
        by_kind("assistant").iter().any(|m| m.content == "on it"),
        "assistant text lost"
    );

    let think = by_kind("thinking");
    assert_eq!(think.len(), 1, "exactly one thinking block");
    assert!(think[0].collapsed, "thinking must be collapsed");
    assert!(
        think[0].content.starts_with("let me think a lot"),
        "thinking content lost: {:?}",
        think[0].content
    );
    assert!(
        think[0].content.ends_with("...") && think[0].content.len() <= 205,
        "thinking must be truncated with ellipsis: len={}",
        think[0].content.len()
    );

    // tool_call — toolName, toolId, collapsed, toolInput preserved, toolArgs summarized (command, ≤80)
    let calls = by_kind("tool_call");
    assert_eq!(calls.len(), 1, "one tool_call");
    let call = calls[0];
    assert_eq!(call.tool_name.as_deref(), Some("Bash"));
    assert_eq!(call.tool_id.as_deref(), Some("u1"));
    assert!(call.collapsed, "tool_call collapsed");
    assert!(
        call.tool_input.is_some(),
        "raw tool_input must survive for diff rendering"
    );
    let args = call.tool_args.as_deref().unwrap_or("");
    assert!(args.starts_with("ls -la"), "toolArgs summary: {args}");
    assert!(
        args.len() <= 80,
        "toolArgs must be capped at 80: {}",
        args.len()
    );

    let results = by_kind("tool_result");
    assert_eq!(results.len(), 2, "both tool_results surfaced: {results:#?}");
    assert!(results
        .iter()
        .any(|m| m.tool_id.as_deref() == Some("u1")
            && m.tool_result.as_deref() == Some("file listing")));
    assert!(results
        .iter()
        .any(|m| m.tool_id.as_deref() == Some("u2")
            && m.tool_result.as_deref() == Some("grep output")));
    assert!(results.iter().all(|m| m.collapsed));

    // user image attachment — on the user message carrying text "see image"
    let with_attach = users
        .iter()
        .find(|m| m.attachments.is_some())
        .expect("user image attachment lost");
    let atts = with_attach.attachments.as_ref().unwrap();
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].mime_type, "image/png");
    assert_eq!(atts[0].data.as_deref(), Some("BASE64DATA"));

    let images = by_kind("image");
    assert_eq!(images.len(), 1, "standalone image lost");
    assert_eq!(images[0].image_mime_type.as_deref(), Some("image/jpeg"));
    assert_eq!(images[0].image_data.as_deref(), Some("IMGBYTES"));
}

#[test]
fn rich_flatten_preserves_command_outcome_facts_and_source_identity() {
    if require_plugins().is_none() {
        return;
    }
    use crate::rich_to_chat::rich_to_chat_msgs;

    let j = load_js("claude");
    let jsonl = concat!(
        r#"{"type":"assistant","uuid":"assistant-command-facts","message":{"role":"assistant","content":[{"type":"tool_use","id":"tool-command-facts","name":"Bash","input":{"command":"printf ok"}}]}}"#,
        "\n",
        r#"{"type":"user","sourceToolAssistantUUID":"assistant-command-facts","toolUseResult":{"returnCode":7,"durationMs":1250,"interrupted":true,"backgroundTaskId":"background-command-facts"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-command-facts","content":"stopped","is_error":true}]}}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    let msgs: Vec<_> = delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
        .collect();
    let call = msgs
        .iter()
        .find(|message| message.kind == "tool_call")
        .expect("command call lost");
    let result = msgs
        .iter()
        .find(|message| message.kind == "tool_result")
        .expect("command result lost");

    assert_eq!(
        call.tool_call_source_id.as_deref(),
        Some("assistant-command-facts")
    );
    assert_eq!(
        result.tool_call_source_id.as_deref(),
        Some("assistant-command-facts")
    );
    assert_eq!(result.tool_pairing.as_deref(), Some("exact"));
    assert_eq!(result.tool_exit_code, Some(7));
    assert_eq!(result.tool_duration_ms, Some(1250));
    assert_eq!(result.tool_interrupted, Some(true));
    assert_eq!(
        result.tool_background_task_id.as_deref(),
        Some("background-command-facts")
    );
}

/// Eval: system entries, isMeta rows, and local-command echoes must
/// normalize to the same system / local_command shapes the desktop
/// parser produced — these need plugin.js to carry uuid/isMeta and emit
/// `system` rows, so this guards that plumbing end-to-end.
#[test]
fn rich_flatten_parity_system_meta_local_command() {
    if require_plugins().is_none() {
        return;
    }
    use crate::rich_to_chat::rich_to_chat_msgs;
    let j = load_js("claude");

    let jsonl = concat!(
        r#"{"type":"system","subtype":"hint","content":"heads up","uuid":"s1"}"#,
        "\n",
        r#"{"type":"user","isMeta":true,"uuid":"m1","message":{"role":"user","content":"Base directory for this skill: /x/y"}}"#,
        "\n",
        r#"{"type":"user","isMeta":true,"uuid":"m2","message":{"role":"user","content":"<system reminder>be nice"}}"#,
        "\n",
        r#"{"type":"user","uuid":"c1","message":{"role":"user","content":"<command-name>compact</command-name>"}}"#,
        "\n",
        r#"{"type":"user","uuid":"u9","message":{"role":"user","content":"hi there"}}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    let msgs: Vec<_> = delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
        .collect();

    let find = |pred: &dyn Fn(&codeterm_types::agent::chat_types::ChatMsg) -> bool| {
        msgs.iter().find(|m| pred(m)).cloned()
    };

    let sys = find(&|m| m.kind == "system" && m.content == "heads up")
        .expect("plain system row lost — plugin.js must emit `system` entries");
    assert_eq!(sys.system_subtype.as_deref(), Some("hint"));

    let skill =
        find(&|m| m.kind == "system" && m.system_subtype.as_deref() == Some("skill_context"))
            .expect("isMeta skill_context row lost");
    assert!(skill.content.contains("Base directory"));

    let meta = find(&|m| m.kind == "system" && m.system_subtype.as_deref() == Some("meta"))
        .expect("isMeta meta row lost");
    assert!(meta.content.contains("be nice"));

    let local = find(&|m| m.kind == "local_command").expect("local-command echo lost");
    assert_eq!(local.tag_name.as_deref(), Some("command-name"));
    assert_eq!(local.content, "compact");

    // streaming mergeById, not a volatile delta index).
    let user =
        find(&|m| m.kind == "user" && m.content == "hi there").expect("normal user turn lost");
    assert!(
        user.id.starts_with("u9-"),
        "id must be uuid-based: {}",
        user.id
    );
}

#[test]
fn rich_flatten_parity_classifies_dante_system_user_rows() {
    if require_plugins().is_none() {
        return;
    }
    use crate::rich_to_chat::rich_to_chat_msgs;
    let j = load_js("claude");

    let jsonl = concat!(
        r#"{"parentUuid":"908feb2c-701a-4612-87b0-f36105adca1a","isSidechain":false,"promptId":"fa97afe2-3473-45a9-8e7f-a12f698367cc","type":"user","message":{"role":"user","content":"This session is being continued from a previous conversation that ran out of context. The summary below covers the earlier portion of the conversation.\n\nSummary:\n1. Primary Request and Intent:\n   - I (Dante, Opus) was made the CodeTerm orchestrator.\n"},"isVisibleInTranscriptOnly":true,"isCompactSummary":true,"uuid":"9c469a99-438b-4051-af05-ab2460f5be37","timestamp":"2026-06-23T02:31:14.443Z","userType":"external","entrypoint":"cli","cwd":"/Users/user/Developer/codeterm-alpha","sessionId":"0b688a01-2ea9-49c8-b045-d2c4b70a09ec","version":"2.1.185","gitBranch":"master","slug":"binary-churning-yao"}"#,
        "\n",
        r#"{"parentUuid":"2945e439-1fd9-4416-8270-3052cd9cc92b","isSidechain":false,"promptId":"7c598803-7747-4e0a-ba9a-d081b2687028","type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>bawoofxfm</task-id>\n<tool-use-id>toolu_015s315pBKC2AjB7FrfdKDy2</tool-use-id>\n<output-file>/private/tmp/claude/tasks/bawoofxfm.output</output-file>\n<status>completed</status>\n<summary>Background command \"Wait for real SSH up (exits only on success)\" completed (exit code 0)</summary>\n</task-notification>"},"uuid":"4cd5c262-c942-455c-b9c0-32d2322da0f6","timestamp":"2026-06-23T05:30:25.000Z","permissionMode":"auto","origin":{"kind":"task-notification"},"promptSource":"system","userType":"external","entrypoint":"cli","cwd":"/Users/user/Developer/codeterm-alpha","sessionId":"0b688a01-2ea9-49c8-b045-d2c4b70a09ec","version":"2.1.185","gitBranch":"fix/web-stream","slug":"binary-churning-yao"}"#,
        "\n",
        r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"real user message"}}"#,
        "\n",
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();

    let delta = j.parse_session_delta(tmp.path(), 0);
    let msgs: Vec<_> = delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(i, r)| rich_to_chat_msgs(r, i))
        .collect();

    assert!(
        msgs.iter()
            .all(|m| !m.content.contains("<task-notification>")),
        "task notification leaked into chat: {msgs:#?}"
    );
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].kind, "system");
    assert_eq!(msgs[0].system_subtype.as_deref(), Some("compact_summary"));
    assert_eq!(
        msgs[0].timeline_marker,
        Some(codeterm_types::agent::chat_types::ChatTimelineMarker::ContextCompaction)
    );
    assert!(msgs[0].collapsed);
    assert_eq!(msgs[0].content, "context compacted");
    assert_eq!(msgs[1].kind, "status");
    assert_eq!(msgs[1].system_subtype.as_deref(), Some("task_notification"));
    assert_eq!(msgs[1].content, "Task completed");
    assert_eq!(msgs[2].kind, "user");
    assert_eq!(msgs[2].content, "real user message");
}

#[test]
fn parse_session_delta_holds_back_partial_line() {
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");

    // One complete line + a truncated second line.
    let jsonl = concat!(
        r#"{"type":"user","message":{"role":"user","content":"first"},"timestamp":"2026-05-20T10:00:00Z"}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"sec"#,
    );
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();
    let delta = j.parse_session_delta(tmp.path(), 0);
    assert_eq!(delta.messages.len(), 1, "partial line must be held back");
    let first_line_end = (jsonl.find('\n').unwrap() + 1) as u64;
    assert_eq!(delta.new_offset, first_line_end);
}

#[test]
fn parse_session_delta_from_nonzero_offset() {
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    // Two lines; we ask for the slice starting at line 2.
    let line1 = r#"{"type":"user","message":{"role":"user","content":"first"}}"#;
    let line2 = r#"{"type":"assistant","message":{"role":"assistant","content":"second"}}"#;
    let jsonl = format!("{}\n{}\n", line1, line2);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &jsonl).unwrap();

    let offset = (line1.len() + 1) as u64;
    let delta = j.parse_session_delta(tmp.path(), offset);
    assert_eq!(delta.messages.len(), 1);
    assert_eq!(delta.messages[0].role, "assistant");
    assert_eq!(delta.new_offset, jsonl.len() as u64);
}

#[test]
fn parse_session_delta_handles_non_ascii_offset_correctly() {
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    // Each line ends in \n. Contains emoji (4 bytes UTF-8 each) so byte counts diverge from char counts.
    let line1 =
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello 🚀 мир 你好\"}}";
    let line2 =
        "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"reply 🎉\"}}";
    let jsonl = format!("{}\n{}\n", line1, line2);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &jsonl).unwrap();
    // First read consumes the whole file. new_offset must equal the BYTE length of jsonl, not a code-unit count.
    let delta = j.parse_session_delta(tmp.path(), 0);
    assert_eq!(
        delta.new_offset,
        jsonl.as_bytes().len() as u64,
        "new_offset must be byte-accurate even with non-ASCII content"
    );
    assert_eq!(delta.messages.len(), 2);
    // Second read from the previous new_offset must return nothing.
    let delta2 = j.parse_session_delta(tmp.path(), delta.new_offset);
    assert_eq!(delta2.messages.len(), 0);
    assert_eq!(delta2.new_offset, jsonl.as_bytes().len() as u64);
}

#[test]
fn parse_session_delta_advances_offset_when_plugin_blows_up() {
    // Defensive: feed the plugin malformed JSONL bytes. Plugin catch-blocks each line, but the test asserts the tailer doesn't get stuck at the same offset.
    if require_plugins().is_none() {
        return;
    }
    let j = load_js("claude");
    let bad = "this is not json at all\nnot json either\n";
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), bad).unwrap();
    let delta = j.parse_session_delta(tmp.path(), 0);
    // No valid messages — but offset must advance to file end so we don't busy-loop on the same bytes.
    assert_eq!(delta.messages.len(), 0);
    assert_eq!(delta.new_offset, bad.as_bytes().len() as u64);
}
