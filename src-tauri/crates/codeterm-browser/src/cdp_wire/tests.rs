use super::*;

fn event(method: &str, session_id: Option<&str>) -> CdpEvent {
    CdpEvent {
        method: method.into(),
        params: json!({}),
        session_id: session_id.map(str::to_string),
    }
}

#[test]
fn browser_cdp_encodes_browser_and_session_requests() {
    let browser: Value =
        serde_json::from_str(&encode_request(1, "Target.getTargets", json!({}), None)).unwrap();
    assert_eq!(
        browser,
        json!({"id": 1, "method": "Target.getTargets", "params": {}})
    );
    let page: Value = serde_json::from_str(&encode_request(
        7,
        "Page.navigate",
        json!({"url": "about:blank"}),
        Some("S1"),
    ))
    .unwrap();
    assert_eq!(page["sessionId"], "S1");
    assert_eq!(page["params"]["url"], "about:blank");
}

#[test]
fn browser_cdp_decodes_result_error_and_event() {
    assert_eq!(
        decode_incoming(r#"{"id":3,"result":{"frameId":"F"}}"#).unwrap(),
        Incoming::Response {
            id: 3,
            result: Ok(json!({"frameId": "F"}))
        }
    );
    assert_eq!(
            decode_incoming(r#"{"id":4,"sessionId":"S","error":{"code":-32000,"message":"No node with given id found"}}"#)
                .unwrap(),
            Incoming::Response {
                id: 4,
                result: Err(ProtocolError {
                    code: -32000,
                    message: "No node with given id found".into(),
                    data: None
                })
            }
        );
    assert_eq!(
        decode_incoming(r#"{"id":5}"#).unwrap(),
        Incoming::Response {
            id: 5,
            result: Ok(json!({}))
        }
    );
    let Incoming::Event(event) = decode_incoming(
        r#"{"method":"Page.screencastFrame","params":{"sessionId":2},"sessionId":"S9"}"#,
    )
    .unwrap() else {
        panic!("expected event");
    };
    assert_eq!(event.method, "Page.screencastFrame");
    assert_eq!(event.session_id.as_deref(), Some("S9"));
    assert_eq!(event.params["sessionId"], 2);
}

#[test]
fn browser_cdp_rejects_malformed_messages() {
    assert!(decode_incoming("not json").is_err());
    assert!(decode_incoming("[1]").is_err());
    assert!(decode_incoming(r#"{"params":{}}"#).is_err());
    assert!(decode_incoming(r#"{"id":"x"}"#).is_err());
}

#[test]
fn browser_cdp_event_filter_scopes_sessions_and_methods() {
    let page = EventFilter {
        session: SessionFilter::SessionOrBrowser("S1".into()),
        methods: Vec::new(),
    };
    assert!(page.matches(&event("Page.loadEventFired", Some("S1"))));
    assert!(page.matches(&event("Target.targetCreated", None)));
    assert!(!page.matches(&event("Page.loadEventFired", Some("S2"))));
    let only_session = EventFilter {
        session: SessionFilter::Session("S1".into()),
        methods: Vec::new(),
    }
    .with_methods(&["Page.screencastFrame", "Runtime."]);
    assert!(only_session.matches(&event("Page.screencastFrame", Some("S1"))));
    assert!(only_session.matches(&event("Runtime.consoleAPICalled", Some("S1"))));
    assert!(!only_session.matches(&event("Page.frameNavigated", Some("S1"))));
    assert!(!only_session.matches(&event("Page.screencastFrame", None)));
    let browser = EventFilter {
        session: SessionFilter::Browser,
        methods: Vec::new(),
    };
    assert!(browser.matches(&event("Target.targetInfoChanged", None)));
    assert!(!browser.matches(&event("Target.targetInfoChanged", Some("S1"))));
    assert!(EventFilter::any().matches(&event("Anything.at", Some("S3"))));
}

#[test]
fn browser_cdp_picks_first_real_page_target() {
    let targets = parse_targets(&json!({"targetInfos": [
        {"targetId": "W", "type": "service_worker", "title": "", "url": "https://a/sw.js", "attached": false},
        {"targetId": "D", "type": "page", "title": "DevTools", "url": "devtools://devtools/bundled/inspector.html", "attached": false},
        {"targetId": "P", "type": "page", "title": "Home", "url": "http://127.0.0.1:1420/", "attached": false, "canAccessOpener": false, "browserContextId": "B"},
        {"broken": true}
    ]}));
    assert_eq!(targets.len(), 3);
    assert_eq!(
        pick_page_target(&targets).map(|t| t.target_id.as_str()),
        Some("P")
    );
    assert_eq!(pick_page_target(&targets[..2]), None);
}

#[test]
fn browser_cdp_user_agent_drops_headless_marker() {
    assert_eq!(
            headful_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/141.0.0.0 Safari/537.36"),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"
        );
}

#[test]
fn browser_cdp_recovers_response_id_after_deep_result() {
    let payload = format!(
        "{{\"result\":{}0{},\"id\":73}}",
        "[".repeat(180),
        "]".repeat(180)
    );
    assert!(decode_incoming(&payload)
        .unwrap_err()
        .contains("recursion limit"));
    assert_eq!(response_id(&payload).unwrap(), Some(73));
    assert_eq!(
        response_id(r#"{"method":"Page.loadEventFired","params":{}}"#).unwrap(),
        None
    );
    assert_eq!(
        response_id(r#"{"id":73,"result":broken}"#).unwrap(),
        Some(73)
    );
}
