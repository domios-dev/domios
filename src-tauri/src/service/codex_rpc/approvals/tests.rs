use super::*;

fn request(kind: &str, params: Value) -> Request {
    Request::new(RequestId::Number(0), kind.into(), params)
}

#[test]
fn codex_rpc_control_approval_reply_mapping() {
    for kind in [
        "item/commandExecution/requestApproval",
        "item/fileChange/requestApproval",
    ] {
        for (index, decision) in ["accept", "acceptForSession", "decline", "cancel"]
            .iter()
            .enumerate()
        {
            let mut route = request(kind, json!({"itemId":"native-item"}));
            assert_eq!(
                route.answer(&(index + 1).to_string()).unwrap(),
                Some(json!({"decision":decision}))
            );
        }
        assert!(request(kind, json!({})).answer("5").is_err());
    }
    let permissions = json!({"network":{"enabled":true}});
    for (value, expected) in [
        ("1", json!({"permissions":permissions,"scope":"turn"})),
        ("2", json!({"permissions":permissions,"scope":"session"})),
        ("3", json!({"permissions":{},"scope":"turn"})),
    ] {
        assert_eq!(
            request(
                "item/permissions/requestApproval",
                json!({"permissions":permissions})
            )
            .answer(value)
            .unwrap(),
            Some(expected)
        );
    }
}

#[test]
fn codex_rpc_control_user_input_collects_each_question_and_free_text() {
    let mut route = request(
        "item/tool/requestUserInput",
        json!({"questions":[
            {"id":"first","question":"opaque-first","options":[{"label":"choice-a"},{"label":"choice-b"}]},
            {"id":"second","question":"opaque-second","options":[]}
        ]}),
    );
    assert!(route.answer("3").is_err());
    assert_eq!(route.answer("2").unwrap(), None);
    assert_eq!(route.prompt().unwrap().question, "opaque-second");
    assert_eq!(
        route.answer("typed answer").unwrap(),
        Some(json!({"answers":{
            "first":{"answers":["choice-b"]},"second":{"answers":["typed answer"]}
        }}))
    );
}

#[test]
fn codex_rpc_control_elicitation_and_dynamic_tools_fail_closed() {
    let mut route = request(
        "mcpServer/elicitation/request",
        json!({"requestedSchema":{"type":"object","properties":{}}}),
    );
    assert!(route.prompt().is_some());
    assert_eq!(
        route.answer("1").unwrap(),
        Some(json!({"action":"accept","content":{}}))
    );
    assert!(request(
        "mcpServer/elicitation/request",
        json!({"requestedSchema":{"type":"object","properties":{"secret":{"type":"string"}}}})
    )
    .prompt()
    .is_none());
    assert!(request("item/tool/call", json!({})).prompt().is_none());
}

#[test]
fn codex_rpc_control_resolved_clears_only_exact_request_and_promotes_next() {
    let tab = "rpc-control-resolved-test";
    REQUESTS.lock().insert(
        tab.into(),
        VecDeque::from([
            request(
                "item/commandExecution/requestApproval",
                json!({"command":"first"}),
            ),
            Request::new(
                RequestId::String("next".into()),
                "item/fileChange/requestApproval".into(),
                json!({"reason":"second"}),
            ),
        ]),
    );
    let first = pending(tab).unwrap();
    remove_request(tab, &RequestId::String("unknown".into()));
    assert_eq!(pending(tab).unwrap(), first);
    remove_request(tab, &RequestId::Number(0));
    let second = pending(tab).unwrap();
    assert_eq!(second.question, "second");
    assert_ne!(first.options, second.options);
    remove_request(tab, &RequestId::String("next".into()));
    assert!(pending(tab).is_none());
    clear_tab(tab);
}

#[test]
fn codex_rpc_control_ui_option_routes_and_numeric_free_text_round_trip() {
    let mut approval = Request::new(
        RequestId::String("wire-route".into()),
        "item/commandExecution/requestApproval".into(),
        json!({}),
    );
    let value = approval.prompt().unwrap().options[1].value.clone();
    assert_eq!(
        approval.answer(&value).unwrap(),
        Some(json!({"decision":"acceptForSession"}))
    );
    let mut question = request(
        "item/tool/requestUserInput",
        json!({"questions":[{"id":"number","question":"opaque","options":[]}]}),
    );
    assert!(!question.prompt().unwrap().options.is_empty());
    assert_eq!(
        question.answer("12345").unwrap(),
        Some(json!({"answers":{"number":{"answers":["12345"]}}}))
    );
}
