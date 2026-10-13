use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct CdpEvent {
    pub method: String,
    pub params: Value,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Response {
        id: u64,
        result: Result<Value, ProtocolError>,
    },
    Event(CdpEvent),
}

pub fn encode_request(id: u64, method: &str, params: Value, session_id: Option<&str>) -> String {
    let mut message = json!({ "id": id, "method": method, "params": params });
    if let (Some(session_id), Some(object)) = (session_id, message.as_object_mut()) {
        object.insert("sessionId".into(), Value::String(session_id.into()));
    }
    message.to_string()
}

pub fn decode_incoming(text: &str) -> Result<Incoming, String> {
    let mut value: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid CDP JSON: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or("CDP message is not an object")?;
    if let Some(id) = object.get("id") {
        let id = id
            .as_u64()
            .ok_or("CDP response id is not an unsigned integer")?;
        let result = match object.remove("error") {
            Some(error) => Err(ProtocolError {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown CDP error")
                    .to_string(),
                data: error.get("data").cloned(),
            }),
            None => Ok(object
                .remove("result")
                .unwrap_or_else(|| Value::Object(Map::new()))),
        };
        return Ok(Incoming::Response { id, result });
    }
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or("CDP message has neither id nor method")?
        .to_string();
    Ok(Incoming::Event(CdpEvent {
        method,
        params: object
            .remove("params")
            .unwrap_or_else(|| Value::Object(Map::new())),
        session_id: object
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionFilter {
    Any,
    Browser,
    Session(String),
    SessionOrBrowser(String),
}

/// `methods` empty means every method; an entry ending in `.` matches a whole domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventFilter {
    pub session: SessionFilter,
    pub methods: Vec<String>,
}

impl EventFilter {
    pub fn any() -> Self {
        Self {
            session: SessionFilter::Any,
            methods: Vec::new(),
        }
    }

    pub fn with_methods(mut self, methods: &[&str]) -> Self {
        self.methods = methods.iter().map(|method| method.to_string()).collect();
        self
    }

    pub fn matches(&self, event: &CdpEvent) -> bool {
        let session = event.session_id.as_deref();
        let session_ok = match &self.session {
            SessionFilter::Any => true,
            SessionFilter::Browser => session.is_none(),
            SessionFilter::Session(id) => session == Some(id.as_str()),
            SessionFilter::SessionOrBrowser(id) => session.is_none_or(|own| own == id),
        };
        session_ok
            && (self.methods.is_empty()
                || self.methods.iter().any(|method| {
                    if method.ends_with('.') {
                        event.method.starts_with(method.as_str())
                    } else {
                        event.method == *method
                    }
                }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub target_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub attached: bool,
}

/// `result` is the `Target.getTargets` response body.
pub fn parse_targets(result: &Value) -> Vec<TargetInfo> {
    result
        .get("targetInfos")
        .and_then(Value::as_array)
        .map(|targets| {
            targets
                .iter()
                .filter_map(|target| serde_json::from_value(target.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn pick_page_target(targets: &[TargetInfo]) -> Option<&TargetInfo> {
    targets
        .iter()
        .find(|target| target.kind == "page" && !target.url.starts_with("devtools://"))
}

/// Headless Chrome advertises `HeadlessChrome`, which search engines answer with a captcha.
pub fn headful_user_agent(user_agent: &str) -> String {
    user_agent.replace("HeadlessChrome", "Chrome")
}

#[cfg(test)]
mod tests {
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
}
