use super::*;

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
