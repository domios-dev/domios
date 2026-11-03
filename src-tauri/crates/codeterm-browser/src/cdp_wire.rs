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

const MAX_DOCUMENT_JSON_DEPTH: usize = 4096;
const DOCUMENT_DECODE_STACK: usize = 64 * 1024 * 1024;

pub(crate) async fn decode_document_incoming(text: String) -> Result<Incoming, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("browser-document-decode".into())
        .stack_size(DOCUMENT_DECODE_STACK)
        .spawn(move || {
            let result = decode_document(&text);
            let _ = sender.send(result);
        })
        .map_err(|error| format!("CDP document decoder could not start: {error}"))?;
    receiver
        .await
        .map_err(|_| "CDP document decoder stopped without a reply".to_string())?
}

fn decode_document(text: &str) -> Result<Incoming, String> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_DOCUMENT_JSON_DEPTH {
                        return Err(format!(
                            "CDP document exceeds maximum JSON depth {MAX_DOCUMENT_JSON_DEPTH}"
                        ));
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    #[derive(Deserialize)]
    struct DocumentReply<'a> {
        id: u64,
        #[serde(borrow)]
        result: Option<&'a serde_json::value::RawValue>,
        #[serde(borrow)]
        error: Option<&'a serde_json::value::RawValue>,
    }
    let reply: DocumentReply<'_> = serde_json::from_str(text)
        .map_err(|error| format!("invalid CDP document envelope: {error}"))?;
    if reply.error.is_some() {
        return decode_incoming(text);
    }
    let result = if let Some(raw) = reply.result {
        let mut decoder = serde_json::Deserializer::from_str(raw.get());
        decoder.disable_recursion_limit();
        Value::deserialize(&mut decoder)
            .map_err(|error| format!("invalid CDP document JSON: {error}"))?
    } else {
        Value::Object(Map::new())
    };
    Ok(Incoming::Response {
        id: reply.id,
        result: Ok(result),
    })
}

pub fn discard_value(value: Value) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(children) => pending.extend(children),
            Value::Object(children) => pending.extend(children.into_values()),
            _ => {}
        }
    }
}

pub(crate) fn response_id(text: &str) -> Result<Option<u64>, String> {
    struct Envelope<'a>(&'a std::cell::Cell<Option<u64>>);
    impl<'de> serde::de::Visitor<'de> for Envelope<'_> {
        type Value = ();

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a CDP message object")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
            while let Some(key) = map.next_key::<String>()? {
                if key == "id" {
                    self.0.set(Some(map.next_value::<u64>()?));
                    return Ok(());
                }
                map.next_value::<serde::de::IgnoredAny>()?;
            }
            Ok(())
        }
    }
    let id = std::cell::Cell::new(None);
    let mut decoder = serde_json::Deserializer::from_str(text);
    let result = serde::Deserializer::deserialize_map(&mut decoder, Envelope(&id));
    if let Some(id) = id.get() {
        return Ok(Some(id));
    }
    result
        .map(|()| None)
        .map_err(|error| format!("invalid CDP envelope: {error}"))
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
mod tests;
