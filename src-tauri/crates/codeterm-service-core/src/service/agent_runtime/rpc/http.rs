use super::*;
use codeterm_plugin::agent_runtime::{
    validate_runtime_reply, AgentRuntimeCapability, AgentRuntimeProtocol, RpcReply, RuntimeHttpSse,
    MAX_RUNTIME_VALUE_BYTES,
};
use reqwest::{redirect::Policy, Url};
use std::collections::BTreeMap;
use tokio::task::JoinSet;

mod requests;
mod sse;

const RESPONSE_BODY_LIMIT: usize = 16 * 1024 * 1024;
const REQUEST_LIMIT: usize = 8;
const RESPONSE_TOO_LARGE: i64 = -32010;

type ResponseFields = BTreeMap<String, Vec<String>>;

pub(in crate::service::agent_runtime) struct Launch {
    pub args: Vec<String>,
    pub password: String,
    pub declaration: RuntimeHttpSse,
    url: Url,
}

impl Launch {
    pub fn prepare(capability: &AgentRuntimeCapability) -> Result<Option<Self>, RuntimeFailure> {
        if capability.runtime == AgentRuntimeProtocol::StdioJsonrpc {
            return Ok(None);
        }
        let declaration = capability
            .http_sse
            .clone()
            .filter(|http| http.valid_for(capability))
            .ok_or_else(|| RuntimeFailure::SpawnFailed {
                reason: "Invalid HTTP runtime declaration".into(),
            })?;
        let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| failure("HTTP port allocation failed"))?;
        let port = reservation
            .local_addr()
            .map_err(|_| failure("HTTP port unavailable"))?
            .port();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/"))
            .map_err(|_| failure("HTTP origin invalid"))?;
        let args = capability
            .command
            .args
            .iter()
            .map(|arg| {
                arg.replace("{httpHost}", "127.0.0.1")
                    .replace("{httpPort}", &port.to_string())
            })
            .collect();
        let password = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        Ok(Some(Self {
            args,
            password,
            declaration,
            url,
        }))
    }
}

fn failure(message: &str) -> RuntimeFailure {
    RuntimeFailure::PipeFailure {
        stream: "http-sse".into(),
        reason: message.into(),
    }
}

pub(in crate::service::agent_runtime) fn attach<R>(stdout: R, launch: Launch) -> Client
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let (client, messages) = Client::channel(None, 16, 256);
    let owner = client.clone();
    let mut ended = client.ended();
    crate::async_runtime::spawn(async move {
        let result = tokio::select! {
            _ = ended.changed() => return,
            result = run(&owner, stdout, launch, messages) => result,
        };
        owner.close(result.err().unwrap_or(RuntimeFailure::PipeEof));
    });
    client
}

async fn run<R: AsyncRead + Unpin>(
    client: &Client,
    stdout: R,
    mut launch: Launch,
    mut messages: mpsc::Receiver<Write>,
) -> Result<(), RuntimeFailure> {
    let mut stdout = BufReader::new(stdout);
    launch.declaration =
        tokio::time::timeout(Duration::from_secs(15), readiness(&mut stdout, &launch))
            .await
            .map_err(|_| RuntimeFailure::Deadline)??;
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| failure("HTTP client unavailable"))?;
    let endpoint = route_url(&launch.url, &launch.declaration.event_path, &Value::Null)?;
    let response = connect_events(&http, &endpoint, &launch).await?;
    let pending_requests = Arc::new(Mutex::new(requests::Requests::default()));
    let mut stream = std::pin::pin!(stream_events(
        response,
        client,
        &http,
        &endpoint,
        &launch,
        &pending_requests,
    ));
    let drain = async {
        let mut bytes = [0u8; 4096];
        loop {
            let count = tokio::io::AsyncReadExt::read(&mut stdout, &mut bytes)
                .await
                .map_err(|_| failure("HTTP process stdout failed"))?;
            if count == 0 {
                return Err::<(), _>(RuntimeFailure::PipeEof);
            }
        }
    };
    tokio::pin!(drain);
    let mut requests = JoinSet::new();
    loop {
        tokio::select! {
            result = &mut stream => return result,
            result = &mut drain => return result,
            finished = requests.join_next(), if !requests.is_empty() => {
                match finished {
                    Some(Ok(Ok(()))) => {},
                    Some(Ok(Err(reason))) => return Err(reason),
                    _ => return Err(failure("HTTP request task failed")),
                }
            }
            message = messages.recv(), if requests.len() < REQUEST_LIMIT => {
                let Some(message) = message else { return Err(RuntimeFailure::PipeEof); };
                let (http, client, origin, declaration, password) = (http.clone(), client.clone(), launch.url.clone(), launch.declaration.clone(), launch.password.clone());
                let pending_requests = pending_requests.clone();
                requests.spawn(async move { execute(&client, &http, &origin, &declaration, &password, &pending_requests, message).await });
            }
        }
    }
}

async fn stream_events(
    response: reqwest::Response,
    client: &Client,
    http: &reqwest::Client,
    endpoint: &Url,
    launch: &Launch,
    pending: &Mutex<requests::Requests>,
) -> Result<(), RuntimeFailure> {
    sse::consume(
        response,
        client,
        &launch.declaration,
        &launch.password,
        pending,
    )
    .await?;
    log::debug!("[runtime-http] SSE disconnected; reconnecting to the same runtime");
    let response = connect_events(http, endpoint, launch).await?;
    client.note_stream_gap();
    sse::consume(
        response,
        client,
        &launch.declaration,
        &launch.password,
        pending,
    )
    .await?;
    Err(failure("SSE disconnected after reconnect"))
}

async fn connect_events(
    http: &reqwest::Client,
    endpoint: &Url,
    launch: &Launch,
) -> Result<reqwest::Response, RuntimeFailure> {
    let response = http
        .get(endpoint.clone())
        .basic_auth(&launch.declaration.username, Some(&launch.password))
        .send()
        .await
        .map_err(|_| failure("SSE handshake failed"))?;
    if !response.status().is_success()
        || !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
    {
        return Err(failure("SSE handshake refused"));
    }
    Ok(response)
}

async fn readiness<R: tokio::io::AsyncBufRead + Unpin>(
    stdout: &mut R,
    launch: &Launch,
) -> Result<RuntimeHttpSse, RuntimeFailure> {
    let mut consumed = 0;
    loop {
        let line = sse::read_line(stdout, 8192)
            .await?
            .ok_or(RuntimeFailure::PipeEof)?;
        consumed += line.len();
        if consumed > 65536 {
            return Err(failure("HTTP readiness output limit"));
        }
        for declaration in
            std::iter::once(&launch.declaration).chain(&launch.declaration.alternatives)
        {
            let expected = format!(
                "{}{}",
                declaration.readiness_prefix,
                launch.url.as_str().trim_end_matches('/')
            );
            if line.trim_end_matches(['\r', '\n']) == expected {
                return Ok(declaration.clone());
            }
        }
    }
}

fn route_url(origin: &Url, template: &str, params: &Value) -> Result<Url, RuntimeFailure> {
    let mut url = origin.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| failure("HTTP origin invalid"))?;
        segments.clear();
        for segment in template.split('/').skip(1) {
            let value = if let Some(name) = segment
                .strip_prefix('{')
                .and_then(|value| value.strip_suffix('}'))
            {
                params
                    .get("path")
                    .and_then(|path| path.get(name))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        RuntimeFailure::PrewriteRefused("HTTP route identity missing".into())
                    })?
            } else {
                segment
            };
            if value.len() > 4096
                || matches!(value, "." | "..")
                || value.contains(['/', '\\'])
                || value.chars().any(char::is_control)
            {
                return Err(RuntimeFailure::PrewriteRefused(
                    "HTTP route identity invalid".into(),
                ));
            }
            segments.push(value);
        }
    }
    if let Some(query) = params.get("query") {
        let query = query
            .as_object()
            .filter(|query| query.len() <= 32)
            .ok_or_else(|| failure("HTTP query invalid"))?;
        let mut pairs = url.query_pairs_mut();
        for (name, value) in query {
            let value = value
                .as_str()
                .filter(|value| value.len() <= 4096)
                .ok_or_else(|| failure("HTTP query value invalid"))?;
            pairs.append_pair(name, value);
        }
    }
    Ok(url)
}

async fn execute(
    client: &Client,
    http: &reqwest::Client,
    origin: &Url,
    declaration: &RuntimeHttpSse,
    password: &str,
    pending: &Mutex<requests::Requests>,
    write: Write,
) -> Result<(), RuntimeFailure> {
    let _attempt = WriteAttempt(write.turn_write.clone());
    let frame: Value =
        serde_json::from_str(&write.text).map_err(|_| failure("HTTP call invalid"))?;
    let reply = frame.get("method").is_none();
    if reply && matches!(frame["error"]["code"].as_i64(), Some(-32601 | -32602)) {
        log::debug!(
            "[runtime-prompt] caller=http_bookkeeping request={} left pending",
            frame["id"]
        );
        write.finish(Ok(()));
        return Ok(());
    }
    let (reply_id, call) = if reply {
        match pending.lock().reply(&frame) {
            Ok((id, call)) => (Some(id), call),
            Err(reason) => {
                write.finish(Err(reason));
                return Ok(());
            }
        }
    } else {
        (None, frame.clone())
    };
    let prepared = (|| {
        let method = call["method"]
            .as_str()
            .ok_or_else(|| RuntimeFailure::PrewriteRefused("HTTP method missing".into()))?;
        let route = declaration
            .routes
            .get(method)
            .ok_or_else(|| RuntimeFailure::PrewriteRefused("HTTP method undeclared".into()))?;
        let url = route_url(origin, &route.path, &call["params"])?;
        let method = reqwest::Method::from_bytes(route.method.as_bytes())
            .map_err(|_| failure("HTTP method invalid"))?;
        let fields = response_fields(&method, &call["params"])?;
        let mut request = http
            .request(method, url)
            .basic_auth(&declaration.username, Some(password))
            .timeout(Duration::from_secs(30));
        if let Some(body) = call["params"].get("body") {
            request = request.json(body);
        }
        request
            .build()
            .map(|request| (request, fields))
            .map_err(|_| RuntimeFailure::PrewriteRefused("HTTP request invalid".into()))
    })();
    let (request, fields) = match prepared.and_then(|request| write.begin(client).map(|_| request))
    {
        Ok(request) => request,
        Err(reason) => {
            write.finish(Err(reason));
            return Ok(());
        }
    };
    if let Some(id) = &reply_id {
        if let Err(reason) = pending.lock().claim(id) {
            write.finish(Err(reason));
            return Ok(());
        }
    }
    let response = match http.execute(request).await {
        Ok(response) => response,
        Err(_) => {
            let reason = failure("HTTP request outcome uncertain; replay refused");
            write.finish(Err(reason.clone()));
            return Err(reason);
        }
    };
    let status = response.status();
    if reply {
        finish_reply(write, status);
        return Ok(());
    }
    let Some(raw_id) = frame.get("id") else {
        write.finish(Ok(()));
        return if status.is_success() {
            Ok(())
        } else {
            Err(failure("HTTP notification outcome uncertain"))
        };
    };
    let id: RpcId =
        serde_json::from_value(raw_id.clone()).map_err(|_| failure("HTTP request id invalid"))?;
    if !status.is_success() {
        write.finish(Ok(()));
        client.ingest(WireFrame::Response {
            id,
            result: Err(RpcFailure {
                code: i64::from(status.as_u16()),
                message: format!("HTTP request refused ({})", status.as_u16()),
                data: None,
            }),
        });
        return Ok(());
    }
    let mut headers = serde_json::Map::new();
    for name in &declaration.response_headers {
        if let Some(value) = response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
        {
            if value.len() > 4096 {
                write.finish(Err(failure("HTTP header limit")));
                return Err(failure("HTTP header limit"));
            }
            headers.insert(name.clone(), json!(value.replace(password, "[redacted]")));
        }
    }
    let result = match read_response(response, headers, fields, password).await {
        Ok(RpcReply::Success { value }) => Ok(value),
        Ok(RpcReply::Error { error }) => Err(error),
        Err(reason) => {
            write.finish(Err(reason.clone()));
            return Err(reason);
        }
    };
    write.finish(Ok(()));
    client.ingest(WireFrame::Response { id, result });
    Ok(())
}

fn response_fields(
    method: &reqwest::Method,
    params: &Value,
) -> Result<Option<ResponseFields>, RuntimeFailure> {
    let Some(fields) = params.get("responseFields") else {
        return Ok(None);
    };
    let valid_name = |name: &str| !name.is_empty() && name.len() <= 64;
    let fields: ResponseFields = serde_json::from_value(fields.clone())
        .ok()
        .filter(|fields: &ResponseFields| {
            *method == reqwest::Method::GET
                && !fields.is_empty()
                && fields.len() <= 16
                && fields.iter().all(|(name, members)| {
                    valid_name(name)
                        && members.len() <= 16
                        && members.iter().all(|member| valid_name(member))
                })
        })
        .ok_or_else(|| RuntimeFailure::PrewriteRefused("HTTP response fields invalid".into()))?;
    Ok(Some(fields))
}

async fn read_response(
    response: reqwest::Response,
    headers: serde_json::Map<String, Value>,
    fields: Option<ResponseFields>,
    password: &str,
) -> Result<RpcReply, RuntimeFailure> {
    let Some(body) = bounded_body(response).await? else {
        return Ok(response_too_large("transport_body", RESPONSE_BODY_LIMIT));
    };
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        prepare_response(&body, headers, fields.as_ref(), &password)
    })
    .await
    .map_err(|_| failure("HTTP response task failed"))?
}

fn prepare_response(
    bytes: &[u8],
    headers: serde_json::Map<String, Value>,
    fields: Option<&ResponseFields>,
    password: &str,
) -> Result<RpcReply, RuntimeFailure> {
    let mut body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes).map_err(|_| failure("HTTP response invalid"))?
    };
    if let (Some(fields), Some(object)) = (fields, body.as_object_mut()) {
        object.retain(|name, value| {
            let Some(members) = fields.get(name) else {
                return false;
            };
            if !members.is_empty() {
                if let Some(items) = value.as_array_mut() {
                    for item in items {
                        if let Some(item) = item.as_object_mut() {
                            item.retain(|key, _| members.contains(key));
                        }
                    }
                }
            }
            true
        });
    }
    redact_value(&mut body, password);
    let value = Value::Object(
        [
            ("body".into(), body),
            ("headers".into(), Value::Object(headers)),
        ]
        .into_iter()
        .collect(),
    );
    let reply = RpcReply::Success { value };
    if validate_runtime_reply(&reply).is_ok() {
        Ok(reply)
    } else {
        Ok(response_too_large("runtime_value", MAX_RUNTIME_VALUE_BYTES))
    }
}

fn response_too_large(boundary: &str, limit: usize) -> RpcReply {
    RpcReply::Error {
        error: RpcFailure {
            code: RESPONSE_TOO_LARGE,
            message: "HTTP response exceeded its byte limit".into(),
            data: Some(json!({"kind":"response_too_large","boundary":boundary,"limitBytes":limit})),
        },
    }
}

fn finish_reply(write: Write, status: reqwest::StatusCode) {
    write.finish(if status.is_success() {
        Ok(())
    } else {
        Err(RuntimeFailure::ReplyRefused {
            status: status.as_u16(),
        })
    });
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Option<Vec<u8>>, RuntimeFailure> {
    if response
        .content_length()
        .is_some_and(|length| length > RESPONSE_BODY_LIMIT as u64)
    {
        return Ok(None);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("HTTP response interrupted"))?
    {
        if body.len().saturating_add(chunk.len()) > RESPONSE_BODY_LIMIT {
            return Ok(None);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

#[cfg(test)]
mod tests;
