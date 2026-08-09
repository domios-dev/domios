//! Composer invariants. Every assertion reads a value the host constructed —
//! never a rendering, and never an inferred count.

use super::*;

fn entry(id: &str, kind: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: kind.into(),
        content: content.into(),
        ..ChatMsg::default()
    }
}

/// `groups` turn groups, the last of which carries a tool_call/tool_result pair.
fn transcript_with_trailing_tool_pair(groups: usize) -> Vec<ChatMsg> {
    let mut entries = Vec::new();
    for index in 0..groups {
        entries.push(entry(&format!("user-{index}"), "user", "ask"));
        entries.push(entry(&format!("assistant-{index}"), "assistant", "answer"));
        entries.push(entry(&format!("tool-call-{index}"), "tool_call", "call"));
        entries.push(entry(
            &format!("tool-result-{index}"),
            "tool_result",
            "result",
        ));
    }
    entries
}

#[test]
fn bounded_window_keeps_whole_turn_groups() {
    let transcript = transcript_with_trailing_tool_pair(4);
    // 4 transcript groups + the in-flight group; a limit of 3 must cut on a
    // group boundary, never between a tool call and its result.
    let composed = compose("brief", &transcript, 3, "next ask");

    assert_eq!(composed.decision.groups_available, 5);
    assert_eq!(composed.decision.groups_sent, 3);
    assert_eq!(composed.decision.groups_dropped, 2);

    for group in &composed.turn.groups {
        let calls = group
            .entries
            .iter()
            .filter(|entry| entry.kind == "tool_call")
            .count();
        let results = group
            .entries
            .iter()
            .filter(|entry| entry.kind == "tool_result")
            .count();
        assert_eq!(
            calls, results,
            "a kept group carries both halves of its tool pair"
        );
    }

    let kept_ids: Vec<&str> = composed
        .turn
        .groups
        .iter()
        .flat_map(|group| group.entries.iter().map(|entry| entry.id.as_str()))
        .collect();
    for index in [2usize, 3] {
        assert!(kept_ids.contains(&format!("tool-call-{index}").as_str()));
        assert!(kept_ids.contains(&format!("tool-result-{index}").as_str()));
    }
    for index in [0usize, 1] {
        assert!(
            composed
                .decision
                .dropped_entry_ids
                .contains(&format!("tool-call-{index}")),
            "a dropped group takes its whole tool pair with it"
        );
        assert!(composed
            .decision
            .dropped_entry_ids
            .contains(&format!("tool-result-{index}")));
        assert!(!kept_ids.contains(&format!("tool-result-{index}").as_str()));
    }
    assert_eq!(
        composed.decision.dropped_entry_ids.len(),
        8,
        "both dropped groups are recorded entry by entry"
    );
}

#[test]
fn bounded_window_never_splits_a_tool_pair_at_any_limit() {
    let transcript = transcript_with_trailing_tool_pair(6);
    for limit in 1..=8 {
        let composed = compose("brief", &transcript, limit, "next ask");
        for group in &composed.turn.groups {
            let has_call = group.entries.iter().any(|e| e.kind == "tool_call");
            let has_result = group.entries.iter().any(|e| e.kind == "tool_result");
            assert_eq!(
                has_call, has_result,
                "limit {limit}: a group is wholly present or wholly absent"
            );
        }
    }
}

#[test]
fn bounded_window_never_drops_the_system_prompt() {
    let brief = "You are CodeTerm. ".repeat(64);
    let transcript = transcript_with_trailing_tool_pair(20);
    let composed = compose(&brief, &transcript, 1, "next ask");

    assert_eq!(
        composed.turn.system, brief,
        "the role brief is a sibling field of the sliced collection"
    );
    assert_eq!(composed.decision.groups_sent, 1);
    assert_eq!(composed.decision.groups_dropped, 20);
    assert!(composed.decision.system_prompt_included);
    assert_eq!(composed.decision.system_prompt_bytes, brief.len());
    assert!(
        !composed
            .turn
            .groups
            .iter()
            .flat_map(|group| group.entries.iter())
            .any(|entry| entry.content == brief),
        "the brief never appears inside a group, so a slice cannot reach it"
    );
}

#[test]
fn the_in_flight_turn_is_never_the_dropped_group() {
    let transcript = transcript_with_trailing_tool_pair(9);
    for limit in [0usize, 1, 2] {
        let composed = compose("brief", &transcript, limit, "the ask being sent");
        let last = composed.turn.groups.last().expect("at least one group");
        assert_eq!(last.entries[0].id, IN_FLIGHT_ENTRY_ID);
        assert_eq!(last.entries[0].content, "the ask being sent");
        assert_eq!(
            composed.decision.limit,
            limit.max(1),
            "the recorded limit is the one in effect"
        );
        assert!(!composed
            .decision
            .dropped_entry_ids
            .iter()
            .any(|id| id == IN_FLIGHT_ENTRY_ID));
    }
}

#[test]
fn an_empty_transcript_composes_the_in_flight_turn_alone() {
    let composed = compose("brief", &[], PROVISIONAL_GROUP_LIMIT, "first ask");
    assert_eq!(composed.decision.groups_available, 1);
    assert_eq!(composed.decision.groups_sent, 1);
    assert_eq!(composed.decision.groups_dropped, 0);
    assert_eq!(composed.decision.entries_sent, 1);
    assert!(composed.decision.dropped_entry_ids.is_empty());
    assert_eq!(composed.decision.outcome, ContextOutcome::Sent);
}

#[test]
fn entries_before_the_first_user_turn_keep_their_own_group() {
    let transcript = vec![
        entry("orphan-1", "assistant", "restored preamble"),
        entry("user-0", "user", "ask"),
        entry("assistant-0", "assistant", "answer"),
    ];
    let composed = compose("brief", &transcript, 12, "next ask");
    assert_eq!(composed.decision.groups_available, 3);
    assert_eq!(composed.turn.groups[0].entries[0].id, "orphan-1");
    assert_eq!(composed.turn.groups[1].entries.len(), 2);
}

#[test]
fn an_oversized_payload_refuses_instead_of_truncating() {
    let huge = "x".repeat(MAX_PAYLOAD_BYTES + 1);
    let transcript = vec![
        entry("user-0", "user", "ask"),
        entry("assistant-0", "assistant", &huge),
    ];
    let composed = compose("brief", &transcript, 12, "next ask");

    assert_eq!(composed.decision.outcome, ContextOutcome::PayloadTooLarge);
    assert!(!composed.is_sendable());
    assert_eq!(
        composed.decision.offending_entry_id.as_deref(),
        Some("assistant-0"),
        "the refusal names the offending entry"
    );
    assert_eq!(
        composed.turn.groups[0].entries[1].content.len(),
        huge.len(),
        "the entry body is never clipped"
    );
}

#[test]
fn the_final_group_is_a_lone_user_entry() {
    // The plugin echoes exactly this group back into its transcript; any other
    // shape and the user's own message stops rendering.
    let composed = compose("brief", &transcript_with_trailing_tool_pair(3), 12, "ask");
    let last = composed.turn.groups.last().unwrap();
    assert_eq!(last.entries.len(), 1);
    assert_eq!(last.entries[0].kind, "user");
    assert_eq!(last.entries[0].content, "ask");
}

#[test]
fn the_role_brief_seed_is_excluded_from_groups_and_recorded() {
    let brief = "You are CodeTerm.";
    let transcript = vec![
        entry(
            "seed-1",
            "user",
            &crate::message_markers::system_prompt(brief),
        ),
        entry("user-0", "user", "ask"),
        entry("assistant-0", "assistant", "answer"),
    ];
    let composed = compose(brief, &transcript, 12, "next ask");

    assert_eq!(
        composed.decision.system_prompt_seed_ids,
        vec!["seed-1".to_string()],
        "the exclusion is a recorded decision, not a silent re-filter"
    );
    assert_eq!(
        composed.decision.groups_available, 2,
        "the seed is not a turn group"
    );
    assert!(
        !composed
            .turn
            .groups
            .iter()
            .flat_map(|group| group.entries.iter())
            .any(|entry| entry.id == "seed-1"),
        "the brief is sent once, in the field built for it — never twice"
    );
    assert_eq!(composed.turn.system, brief);
}

#[test]
fn an_absent_brief_is_promoted_from_the_seed_entry() {
    let brief = "You are CodeTerm.";
    let transcript = vec![
        entry(
            "seed-1",
            "user",
            &crate::message_markers::system_prompt(brief),
        ),
        entry("user-0", "user", "ask"),
    ];
    // No `_meta` brief: the seed's payload moves into the field a slice cannot
    // reach, rather than being dropped with the entry.
    let composed = compose("", &transcript, 12, "next ask");

    assert_eq!(composed.turn.system, brief);
    assert!(composed.decision.system_prompt_included);
    assert_eq!(composed.decision.system_prompt_bytes, brief.len());
}

#[test]
fn a_quoted_marker_inside_a_user_message_is_not_the_seed() {
    let quoted = format!(
        "look at this: {}",
        crate::message_markers::system_prompt("You are CodeTerm.")
    );
    let transcript = vec![entry("user-0", "user", &quoted)];
    let composed = compose("brief", &transcript, 12, "next ask");

    assert!(
        composed.decision.system_prompt_seed_ids.is_empty(),
        "only a host-injected leading marker is the seed"
    );
    assert_eq!(composed.decision.groups_available, 2);
    assert_eq!(composed.turn.groups[0].entries[0].content, quoted);
}

#[test]
fn the_decision_records_messages_as_the_unit() {
    let composed = compose("brief", &transcript_with_trailing_tool_pair(2), 12, "ask");
    let line = serde_json::to_value(&composed.decision).unwrap();
    assert_eq!(line["unit"], "messages");
    assert_eq!(line["entriesSent"], 9);
    // Exact schema: a counting field of any other unit could not be added
    // without this failing.
    let mut keys = line.as_object().unwrap().keys().collect::<Vec<_>>();
    keys.sort();
    let mut expected = vec![
            "turn",
            "unit",
            "limit",
            "groupsAvailable",
            "groupsSent",
            "groupsDropped",
            "entriesSent",
            "droppedEntryIds",
            "systemPromptIncluded",
            "systemPromptBytes",
            "systemPromptSeedIds",
            "payloadBytes",
            "outcome",
    ];
    expected.sort();
    assert_eq!(keys, expected);
}
