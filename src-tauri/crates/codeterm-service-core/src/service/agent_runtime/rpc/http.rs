use super::*;
use codeterm_plugin::agent_runtime::{
    AgentRuntimeCapability, AgentRuntimeProtocol, RuntimeHttpSse,
};
use reqwest::{redirect::Policy, Url};
use tokio::task::JoinSet;

mod requests;
mod sse;

const BODY_LIMIT: usize = codeterm_plugin::agent_runtime::MAX_RUNTIME_VALUE_BYTES;
const REQUEST_LIMIT: usize = 8;

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
    let response = http
        .get(endpoint)
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
    let pending_requests = Arc::new(Mutex::new(requests::Requests::default()));
    let stream = sse::consume(
        response,
        client,
        &launch.declaration,
        &launch.password,
        &pending_requests,
    );
    tokio::pin!(stream);
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
        let mut request = http
            .request(method, url)
            .basic_auth(&declaration.username, Some(password))
            .timeout(Duration::from_secs(30));
        if let Some(body) = call["params"].get("body") {
            request = request.json(body);
        }
        request
            .build()
            .map_err(|_| RuntimeFailure::PrewriteRefused("HTTP request invalid".into()))
    })();
    let request = match prepared.and_then(|request| write.begin(client).map(|_| request)) {
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
    let body = bounded_body(response).await;
    if let Err(reason) = &body {
        write.finish(Err(reason.clone()));
        return Err(reason.clone());
    }
    let mut value = body?;
    redact_value(&mut value, password);
    write.finish(Ok(()));
    if frame.get("id").is_some() {
        let id: RpcId = serde_json::from_value(frame["id"].clone())
            .map_err(|_| failure("HTTP request id invalid"))?;
        let result = if status.is_success() {
            Ok(json!({"body":value,"headers":headers}))
        } else {
            Err(RpcFailure {
                code: i64::from(status.as_u16()),
                message: format!("HTTP request refused ({})", status.as_u16()),
                data: None,
            })
        };
        client.ingest(WireFrame::Response { id, result });
    } else if !status.is_success() {
        return Err(failure("HTTP notification outcome uncertain"));
    }
    Ok(())
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

async fn bounded_body(mut response: reqwest::Response) -> Result<Value, RuntimeFailure> {
    if response
        .content_length()
        .is_some_and(|length| length > BODY_LIMIT as u64)
    {
        return Err(failure("HTTP body limit"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("HTTP response interrupted"))?
    {
        if body.len().saturating_add(chunk.len()) > BODY_LIMIT {
            return Err(failure("HTTP body limit"));
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&body).map_err(|_| failure("HTTP response invalid"))
}

#[cfg(test)]
mod tests;
