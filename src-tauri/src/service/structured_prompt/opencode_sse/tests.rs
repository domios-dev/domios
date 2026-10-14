use super::*;
use serde_json::json;

#[test]
fn turn_model_requires_assistant_metadata_without_losing_the_message() {
    let captured: Value =
        serde_json::from_str(include_str!("../fixtures/captured_message_updated.json")).unwrap();
    for (field, value) in [
        ("role", json!("user")),
        ("providerID", Value::Null),
        ("modelID", Value::Null),
        ("providerID", json!(" ")),
        ("modelID", json!("")),
    ] {
        let mut event = captured.clone();
        event["properties"]["info"][field] = value;
        let ParsedEvent::MessageUpdated { message } = parse_event(&event) else {
            panic!("model metadata must not gate transcript ingestion");
        };
        assert_eq!(message.message_id, "msg_1");
        assert_eq!(message.model, None);
    }
}

#[test]
fn completed_tool_diffs_match_plugin_fixtures_and_reject_unfinished_edits() {
    let fixtures: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/opencode-file-diffs.json"
    )))
    .unwrap();
    let provider = diff_provider();
    for fixture in fixtures.as_array().unwrap() {
        for status in ["completed", "running", "pending", "error"] {
            let update = ChatPartUpdate {
                session_id: "own".into(),
                message_id: "message".into(),
                part_id: "part".into(),
                delta: None,
                event: ChatPartEvent::Updated,
                part: json!({"type":"tool","tool":"edit","state":{
                    "status":status,"input":fixture["input"],"metadata":fixture["metadata"]
                }}),
            };
            let message = MessageMeta {
                model: None,
                session_id: "own".into(),
                message_id: "message".into(),
                role: "assistant".into(),
                timestamp: None,
            };
            let mut result = normalize_chat_part(&update, &message, None).unwrap();
            super::super::tool_diffs::map(&provider, &update.part, &mut result);
            if status == "completed" {
                assert_eq!(
                    result.file_diffs,
                    Some(fixture["expected"].clone()),
                    "{}",
                    fixture["name"]
                );
                assert_eq!(result.tool_error, Some(false));
            } else {
                assert_eq!(result.file_diffs, None);
                assert_eq!(
                    result.tool_error,
                    if status == "error" { Some(true) } else { None }
                );
            }
        }
    }
}

#[test]
fn parses_question_asked_into_prompt_and_route() {
    // Real shape from the W3 spike doc.
    let event = json!({
        "id": "evt_1",
        "type": "question.asked",
        "properties": {
            "id": "que_42",
            "sessionID": "ses_9",
            "questions": [{
                "question": "Which option should CodeTerm choose?",
                "header": "Spike",
                "options": [
                    {"label": "Alpha", "description": ""},
                    {"label": "Beta", "description": ""},
                    {"label": "Gamma", "description": ""}
                ]
            }]
        }
    });
    match parse_event(&event) {
        ParsedEvent::Prompt { prompt, route } => {
            assert_eq!(prompt.question, "Which option should CodeTerm choose?");
            assert_eq!(prompt.options.len(), 4);
            assert_eq!(prompt.options[0].label, "Alpha");
            assert_eq!(prompt.options[0].value, "1");
            assert_eq!(prompt.multi_select, Some(false));
            assert_eq!(route.kind, PromptKind::Question);
            assert_eq!(route.request_id, "que_42");
            assert_eq!(route.session_id, "ses_9");
            assert_eq!(reply_endpoint(&route), "/question/que_42/reply");
            assert_eq!(
                reply_body(&route, "1").unwrap(),
                json!({"answers": [["Alpha"]]})
            );
            assert_eq!(
                reply_body(&route, "3").unwrap(),
                json!({"answers": [["Gamma"]]})
            );
            assert!(reply_body(&route, "9").is_none());
        }
        other => panic!("expected Prompt, got {other:?}"),
    }
}

#[test]
fn parses_permission_asked_with_three_replies() {
    let event = json!({
        "id": "evt_2",
        "type": "permission.asked",
        "properties": {
            "id": "per_7",
            "sessionID": "ses_5",
            "permission": "bash",
            "metadata": {
                "command": "printf X",
                "description": "Print X string"
            }
        }
    });
    match parse_event(&event) {
        ParsedEvent::Prompt { prompt, route } => {
            assert_eq!(prompt.question, "Print X string");
            let values: Vec<_> = prompt.options.iter().map(|o| o.value.as_str()).collect();
            assert_eq!(values, vec!["once", "always", "reject"]);
            assert_eq!(route.kind, PromptKind::Permission);
            assert_eq!(reply_endpoint(&route), "/permission/per_7/reply");
            assert_eq!(reply_body(&route, "1").unwrap(), json!({"reply": "once"}));
            assert_eq!(
                reply_body(&route, "always").unwrap(),
                json!({"reply": "always"})
            );
            assert!(reply_body(&route, "4").is_none());
        }
        other => panic!("expected Prompt, got {other:?}"),
    }
}

#[test]
fn permission_falls_back_to_command_then_permission_label() {
    let event = json!({
        "type": "permission.asked",
        "properties": {
            "id": "per_8", "sessionID": "ses_5",
            "permission": "edit",
            "metadata": {"command": "apply patch"}
        }
    });
    let ParsedEvent::Prompt { prompt, .. } = parse_event(&event) else {
        panic!("expected prompt");
    };
    assert_eq!(prompt.question, "apply patch");
}

#[test]
fn replied_event_maps_to_clear() {
    let event = json!({
        "type": "question.replied",
        "properties": {"sessionID": "ses_9", "requestID": "que_42", "answers": [["Alpha"]]}
    });
    assert_eq!(
        parse_event(&event),
        ParsedEvent::Replied {
            request_id: "que_42".into()
        }
    );
}

#[test]
fn rejected_question_maps_to_clear_by_request_id() {
    assert_eq!(
        parse_event(&json!({
            "type": "question.rejected",
            "properties": {"sessionID": "ses_9", "requestID": "que_42"}
        })),
        ParsedEvent::Replied {
            request_id: "que_42".into()
        }
    );
    assert_eq!(
        parse_event(&json!({
            "type": "question.rejected",
            "properties": {"sessionID": "ses_9"}
        })),
        ParsedEvent::Ignored
    );
}

#[test]
fn server_connected_and_unknown_are_ignored() {
    let observed: Value =
        serde_json::from_str(include_str!("../fixtures/observed_server_connected.json"))
            .expect("captured server.connected frame");
    assert_eq!(parse_event(&observed), ParsedEvent::Ignored);
    assert_eq!(
        parse_event(&json!({"type": "unrecognized.event"})),
        ParsedEvent::Ignored
    );
}

#[test]
fn text_delta_frames_omit_kind_and_full_part_frames_normalize() {
    let message_frame: Value =
        serde_json::from_str(include_str!("../fixtures/captured_message_updated.json")).unwrap();
    let message = parse_event(&message_frame);
    let ParsedEvent::MessageUpdated { message } = message else {
        panic!("expected message metadata");
    };
    assert_eq!(message.message_id, "msg_1");
    assert_eq!(message.session_id, "ses_1");
    assert_eq!(message.role, "assistant");
    assert_eq!(message.timestamp.as_deref(), Some("1789814516718"));
    assert_eq!(message.model.as_deref(), Some("deepseek/deepseek-v4-pro"));

    let first_frame: Value = serde_json::from_str(include_str!(
        "../fixtures/captured_message_part_delta_first.json"
    ))
    .unwrap();
    let first = parse_event(&first_frame);
    let ParsedEvent::PartUpdated { update } = first else {
        panic!("expected part delta");
    };
    assert_eq!(update.part_id, "prt_text");
    assert_eq!(update.delta.as_deref(), Some("121"));
    assert_eq!(update.event, ChatPartEvent::Delta);
    assert!(update.part.get("type").is_none());

    let second_frame: Value = serde_json::from_str(include_str!(
        "../fixtures/captured_message_part_delta_second.json"
    ))
    .unwrap();
    let second = parse_event(&second_frame);
    let ParsedEvent::PartUpdated { update } = second else {
        panic!("expected part delta");
    };
    assert_eq!(update.delta.as_deref(), Some("932"));
    assert!(update.part.get("type").is_none());

    let full_frame: Value = serde_json::from_str(include_str!(
        "../fixtures/captured_message_part_updated_text.json"
    ))
    .unwrap();
    let ParsedEvent::PartUpdated { update } = parse_event(&full_frame) else {
        panic!("expected completed text part");
    };
    assert_eq!(update.delta, None);
    assert_eq!(update.event, ChatPartEvent::Updated);
    let completed = normalize_chat_part(&update, &message, None).unwrap();
    assert_eq!(completed.id, "prt_text");
    assert_eq!(completed.content, "121932631112635269");
}

#[test]
fn live_reasoning_and_tool_lifecycle_keep_their_part_identity() {
    let message = MessageMeta {
        model: None,
        session_id: "ses_2".into(),
        message_id: "msg_2".into(),
        role: "assistant".into(),
        timestamp: Some("1789814516718".into()),
    };
    let reasoning_frame: Value = serde_json::from_str(include_str!(
        "../fixtures/captured_message_part_reasoning.json"
    ))
    .unwrap();
    let ParsedEvent::PartUpdated { update: reasoning } = parse_event(&reasoning_frame) else {
        panic!("expected reasoning part update");
    };
    let thinking = normalize_chat_part(&reasoning, &message, None).unwrap();
    assert_eq!(thinking.kind, "thinking");
    assert_eq!(thinking.content, "<redacted reasoning text>");

    let pending = parse_tool_fixture(
        include_str!("../fixtures/captured_message_part_tool_pending.json"),
        &message,
    );
    let running = parse_tool_fixture(
        include_str!("../fixtures/captured_message_part_tool_running.json"),
        &message,
    );
    let completed = parse_tool_fixture(
        include_str!("../fixtures/captured_message_part_tool_completed.json"),
        &message,
    );
    assert_eq!(pending.id, "prt_tool");
    assert_eq!(pending.id, running.id);
    assert_eq!(running.id, completed.id);
    assert_eq!(pending.kind, "tool_call");
    assert_eq!(
        running.tool_input.as_ref().unwrap()["command"],
        "<redacted bash command>"
    );
    assert_eq!(completed.tool_name.as_deref(), Some("bash"));
    assert_eq!(completed.tool_id.as_deref(), Some("<redacted call>"));
    assert_eq!(completed.tool_result.as_deref(), Some("<redacted output>"));
    assert_eq!(
        completed.tool_title.as_deref(),
        Some("<redacted bash command>")
    );
    assert_eq!(completed.tool_exit_code, Some(0));
    assert_eq!(completed.tool_duration_ms, Some(102_042));
    assert_eq!(completed.timestamp.as_deref(), Some("1789814520476"));
}

fn parse_tool_fixture(frame: &str, message: &MessageMeta) -> ChatMsg {
    let frame: Value = serde_json::from_str(frame).unwrap();
    let ParsedEvent::PartUpdated { update } = parse_event(&frame) else {
        panic!("expected tool part update");
    };
    normalize_chat_part(&update, message, None).unwrap()
}

#[test]
fn part_update_requires_matching_message_and_session() {
    let message = MessageMeta {
        model: None,
        session_id: "ses_owner".into(),
        message_id: "msg_owner".into(),
        role: "assistant".into(),
        timestamp: None,
    };
    let part = ChatPartUpdate {
        session_id: "ses_sibling".into(),
        message_id: "msg_owner".into(),
        part_id: "prt_sibling".into(),
        part: json!({"type":"text", "text":"sibling"}),
        delta: Some("sibling".into()),
        event: ChatPartEvent::Updated,
    };
    assert!(normalize_chat_part(&part, &message, None).is_none());
}

#[test]
fn parses_live_permission_asked_and_replied_shapes() {
    let event: Value =
        serde_json::from_str(include_str!("../fixtures/captured_permission_asked.json")).unwrap();
    let ParsedEvent::Prompt { prompt, route } = parse_event(&event) else {
        panic!("expected prompt");
    };
    assert_eq!(prompt.question, "<redacted bash command>");
    assert_eq!(route.request_id, "per_probe");
    assert_eq!(route.session_id, "ses_5");
    assert_eq!(
        parse_event(&json!({
            "type": "permission.updated",
            "properties": {"id": "legacy", "sessionID": "ses_5"}
        })),
        ParsedEvent::Ignored
    );
    assert_eq!(
        parse_event(
            &serde_json::from_str(include_str!("../fixtures/captured_permission_replied.json"))
                .unwrap()
        ),
        ParsedEvent::Replied {
            request_id: "per_probe".into()
        }
    );
}

#[test]
fn multi_select_question_marks_submit_unavailable() {
    let event = json!({
        "type": "question.asked",
        "properties": {
            "id": "que_1", "sessionID": "ses_1",
            "questions": [{
                "question": "Pick many", "multiple": true,
                "options": [{"label": "A"}, {"label": "B"}]
            }]
        }
    });
    let ParsedEvent::Prompt { prompt, route } = parse_event(&event) else {
        panic!("expected prompt");
    };
    assert_eq!(prompt.multi_select, Some(true));
    assert_eq!(prompt.submit_available, Some(false));
    // The multi-select route still carries only ordered labels — see the GAP test.
    assert_eq!(route.option_labels, vec!["A".to_string(), "B".to_string()]);
}

#[test]
fn question_reply_body_is_label_based_index_and_label_equivalent() {
    let route = ResponseRoute {
        session_id: "ses_x".into(),
        request_id: "que_x".into(),
        kind: PromptKind::Question,
        option_labels: vec!["Alpha".into(), "Beta".into()],
        questions: vec![crate::service::structured_prompt::questions::Question {
            header: "H".into(),
            text: "Q".into(),
            labels: vec!["Alpha".into(), "Beta".into()],
            multiple: false,
            custom: false,
        }],
        answers: Vec::new(),
    };
    assert_eq!(
        reply_body(&route, "1").unwrap(),
        json!({"answers": [["Alpha"]]})
    );
    assert_eq!(
        reply_body(&route, "Alpha").unwrap(),
        json!({"answers": [["Alpha"]]}),
        "label resolves identically to its 1-based index"
    );
    assert_eq!(
        reply_body(&route, "2").unwrap(),
        json!({"answers": [["Beta"]]})
    );
    assert!(
        reply_body(&route, "3").is_none(),
        "out-of-range index -> None"
    );
    assert!(
        reply_body(&route, "Gamma").is_none(),
        "unknown label -> None"
    );
}

#[test]
fn multi_select_question_reply_body_contains_every_selected_choice() {
    let route = ResponseRoute {
        session_id: "ses_m".into(),
        request_id: "que_m".into(),
        kind: PromptKind::Question,
        option_labels: vec!["A".into(), "B".into(), "C".into()],
        questions: vec![crate::service::structured_prompt::questions::Question {
            header: "H".into(),
            text: "Q".into(),
            labels: vec!["A".into(), "B".into(), "C".into()],
            multiple: true,
            custom: false,
        }],
        answers: Vec::new(),
    };
    assert_eq!(
        reply_body(&route, "10005").unwrap(),
        json!({"answers": [["A", "C"]]})
    );
    assert_eq!(
        reply_body(&route, "3").unwrap(),
        json!({"answers": [["C"]]})
    );
}

#[test]
fn lifecycle_and_todo_events_require_a_session_and_preserve_payloads() {
    for (event, kind) in [
        ("session.compacted", SessionUpdate::Compacted),
        (
            "session.error",
            SessionUpdate::Error {
                error: json!({"message":"failure"}),
            },
        ),
        (
            "todo.updated",
            SessionUpdate::Todos {
                todos: vec![json!({"content":"task","status":"pending"})],
            },
        ),
    ] {
        assert_eq!(
            parse_event(
                &json!({"type":event,"properties":{"sessionID":"own","error":{"message":"failure"},"todos":[{"content":"task","status":"pending"}]}})
            ),
            ParsedEvent::SessionUpdate {
                session_id: "own".into(),
                kind
            }
        );
        assert_eq!(
            parse_event(&json!({"type":event,"properties":{}})),
            ParsedEvent::Ignored
        );
    }
}

#[test]
fn v2_question_events_keep_their_session_scoped_routes() {
    let ParsedEvent::Prompt { route, .. } =
        parse_event(&json!({"type":"question.v2.asked","properties":{
            "id":"q2","sessionID":"s2","questions":[{"question":"Q","options":[{"label":"A"}]}]
        }}))
    else {
        panic!("expected question");
    };
    assert_eq!(reply_endpoint(&route), "/api/session/s2/question/q2/reply");
    assert_eq!(reply_body(&route, "1"), Some(json!({"answers":[["A"]]})));
    for event in ["question.v2.replied", "question.v2.rejected"] {
        assert_eq!(
            parse_event(&json!({"type":event,"properties":{"sessionID":"s2","requestID":"q2"}})),
            ParsedEvent::Replied {
                request_id: "q2".into()
            }
        );
    }
    let ParsedEvent::Prompt { route, .. } =
        parse_event(&json!({"type":"permission.v2.asked","properties":{
            "id":"p2","sessionID":"s2","action":"shell","resources":[]
        }}))
    else {
        panic!("expected permission");
    };
    assert_eq!(
        reply_endpoint(&route),
        "/api/session/s2/permission/p2/reply"
    );
    assert_eq!(reply_body(&route, "once"), Some(json!({"reply":"once"})));
}

fn diff_provider() -> crate::plugin::js_runtime::JsProvider {
    let manifest = crate::test_support::fixture_manifest_dir()
        .join("../plugins/provider-opencode/plugin.json");
    crate::plugin::js_runtime::loader::load_one(&manifest).unwrap()
}

#[test]
fn stored_opencode_tool_parts_map_to_chat_file_diffs_through_the_plugin() {
    let provider = diff_provider();
    let fixtures: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/opencode-1.18.35-tool-parts.json"
    )))
    .unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let update = ChatPartUpdate {
            session_id: "own".into(),
            message_id: "message".into(),
            part_id: "part".into(),
            part: fixture["part"].clone(),
            delta: None,
            event: ChatPartEvent::Updated,
        };
        let meta = MessageMeta {
            model: None,
            session_id: "own".into(),
            message_id: "message".into(),
            role: "assistant".into(),
            timestamp: None,
        };
        let mut row = normalize_chat_part(&update, &meta, None).unwrap();
        super::super::tool_diffs::map(&provider, &update.part, &mut row);
        let diff = &row.file_diffs.as_ref().unwrap()[0];
        assert_eq!(diff["path"], fixture["expectedPath"]);
        assert_eq!(diff["additions"], fixture["additions"]);
        assert_eq!(diff["deletions"], fixture["deletions"]);
        assert_eq!(row.tool_error, Some(false));
        assert_eq!(row.id, "part");
    }
}
