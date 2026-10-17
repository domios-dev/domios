use super::*;

#[test]
fn host_context_markup_round_trips_as_inert_json() {
    let context = serde_json::json!({"task": "<pasted_content id=\"opaque\">λ</pasted_content id=\"opaque\">"});
    let encoded = inert_json(&context.to_string());
    assert!(!encoded.contains(['<', '>']));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
        context
    );
}

#[test]
fn historical_markup_is_inert_and_round_trips_without_transport_tags() {
    let original =
        "<pasted_content id=\"opaque\">\n<domios>λ\\\"</domios>\n</pasted_content id=\"opaque\">";
    let literal = history_literal(original);
    assert!(!literal.contains(['<', '>']));
    assert_eq!(serde_json::from_str::<String>(&literal).unwrap(), original);
}

#[test]
fn encoded_history_stays_in_its_byte_budget_under_maximal_expansion() {
    for content in [
        "<".repeat(5000),
        "\u{0001}".repeat(5000),
        "λ\\\"\n".repeat(5000),
    ] {
        let messages = vec![ChatMsg {
            kind: "user".into(),
            content,
            ..Default::default()
        }];
        for budget in 0..20 {
            assert!(select_history_literal(&messages, budget).len() <= budget);
        }
        let literal = select_history_literal(&messages, 2000);
        assert!(literal.len() <= 2000);
        assert!(!serde_json::from_str::<String>(&literal).unwrap().is_empty());
    }
}

#[test]
fn handoff_fragments_round_trip_in_order() {
    let fragments = HandoffFragments {
        instructions: "α",
        context: "β",
        history: "γ",
    };
    let result = assemble(fragments).unwrap();
    assert_eq!(result.split("\n\n").collect::<Vec<_>>(), ["α", "β", "γ"]);
}

#[test]
fn handoff_bound_counts_utf8_bytes_and_all_fragments() {
    let exact = "é".repeat((MAX_INPUT_BYTES - 4) / 2);
    assert_eq!(
        assemble(HandoffFragments {
            instructions: &exact,
            context: "",
            history: ""
        })
        .unwrap()
        .len(),
        MAX_INPUT_BYTES
    );
    assert!(assemble(HandoffFragments {
        instructions: &exact,
        context: "x",
        history: ""
    })
    .is_err());
}

#[test]
fn handoff_selects_recent_dialogue_in_chronological_order() {
    let mut messages: Vec<_> = (0..20)
        .map(|index| ChatMsg {
            kind: if index % 2 == 0 { "user" } else { "assistant" }.into(),
            content: index.to_string(),
            ..Default::default()
        })
        .collect();
    messages.push(ChatMsg {
        kind: "assistant".into(),
        content: "tool".into(),
        tool_result: Some("output".into()),
        ..Default::default()
    });
    let selected = recent_messages(&messages);
    assert_eq!(selected.len(), MAX_RECENT_MESSAGES);
    assert_eq!(
        selected
            .iter()
            .map(|message| message.content.clone())
            .collect::<Vec<_>>(),
        (8..20).map(|index| index.to_string()).collect::<Vec<_>>()
    );
}
