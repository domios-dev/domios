use super::{auth::is_local_caller, routes::AppState};
use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, MethodRouter},
    Json, Router,
};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

type ApiError = (StatusCode, String);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/frontend/bootstrap", get(bootstrap))
        .route("/api/frontend/ready", post(ready))
        .route(
            "/api/frontend/diagnostic",
            bounded_diagnostic(
                post(diagnostic),
                Arc::new(DiagnosticAdmission::new(Arc::new(Instant::now))),
            ),
        )
        .route("/api/desktop/recovery", get(recovery))
        .route("/api/listener/status", get(listener_status))
        .route("/api/listener/restart", post(listener_restart))
}

const MAX_DIAGNOSTIC_CLIENTS: usize = 1024;
const DIAGNOSTIC_WINDOW: Duration = Duration::from_secs(60);

#[derive(Default)]
struct DiagnosticWindows {
    entries: HashMap<(String, Option<IpAddr>), (Instant, u32)>,
}

impl DiagnosticWindows {
    fn admit(
        &mut self,
        client: &str,
        remote: Option<IpAddr>,
        now: Instant,
    ) -> Result<(), StatusCode> {
        self.entries
            .retain(|_, (start, _)| now.saturating_duration_since(*start) < DIAGNOSTIC_WINDOW);
        let key = (client.to_owned(), remote);
        if !self.entries.contains_key(&key) && self.entries.len() >= MAX_DIAGNOSTIC_CLIENTS {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let (_, count) = self.entries.entry(key).or_insert((now, 0));
        if *count >= 60 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        *count += 1;
        Ok(())
    }
}

struct DiagnosticAdmission {
    windows: Mutex<DiagnosticWindows>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl DiagnosticAdmission {
    fn new(clock: Arc<dyn Fn() -> Instant + Send + Sync>) -> Self {
        Self {
            windows: Mutex::new(DiagnosticWindows::default()),
            clock,
        }
    }
}

fn diagnostic_client(headers: &HeaderMap) -> &str {
    headers
        .get("X-CodeTerm-Client-Id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
        .unwrap_or("unknown")
}

fn bounded_diagnostic<S: Clone + Send + Sync + 'static>(
    route: MethodRouter<S>,
    admission: Arc<DiagnosticAdmission>,
) -> MethodRouter<S> {
    route
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(
            admission,
            diagnostic_admission,
        ))
}

async fn diagnostic_admission(
    State(admission): State<Arc<DiagnosticAdmission>>,
    request: Request,
    next: Next,
) -> Response {
    if !cfg!(debug_assertions) {
        return StatusCode::CONFLICT.into_response();
    }
    let remote = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0.ip());
    let result = admission.windows.lock().admit(
        diagnostic_client(request.headers()),
        remote,
        (admission.clock)(),
    );
    if let Err(status) = result {
        return status.into_response();
    }
    next.run(request).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DiagnosticStage {
    BootstrapInjected,
    BootstrapConsumed,
    BootstrapMissing,
    BootstrapRestored,
    BootstrapInvalid,
    RuntimeBound,
    ModuleEntry,
    PageFinished,
    FrontendInitialize,
    FrontendInitialized,
    FrontendInitializeFailed,
    ReadyRequest,
    ReadyAccepted,
    ReadyRejected,
    ReadyFailed,
    ScriptError,
    PromiseError,
    RequestFailed,
    ChatScroll,
}

#[derive(Debug, Deserialize)]
enum DiagnosticError {
    Error,
    ReferenceError,
    TypeError,
    SyntaxError,
    SecurityError,
    FrontendUpdateError,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BootFailureReason {
    InvalidClient,
    CheckpointUnavailable,
    StatusUnavailable,
    ApiIncompatible,
    StorageUnavailable,
    BootContextInvalid,
    Unexpected,
    NativeCapabilityDenied,
    RejectedString,
    RejectedValue,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BootDiagnostic {
    stage: DiagnosticStage,
    request: Option<RequestFailure>,
    chat: Option<Value>,
    error_name: Option<DiagnosticError>,
    #[serde(default, deserialize_with = "diagnostic_reason")]
    reason: Option<String>,
    http_status: Option<u16>,
    source: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    native: bool,
    injected: bool,
    mounted: bool,
}

fn diagnostic_reason<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(reason) = &value {
        let (base, dropped) = reason
            .split_once("; dropped=")
            .map_or((reason.as_str(), None), |(base, count)| (base, Some(count)));
        serde_json::from_value::<BootFailureReason>(Value::String(base.into()))
            .map_err(serde::de::Error::custom)?;
        if let Some(count) = dropped {
            if count.is_empty()
                || count.len() > 10
                || !count.bytes().all(|byte| byte.is_ascii_digit())
                || count.parse::<u32>().is_err()
            {
                return Err(serde::de::Error::custom("Invalid diagnostic drop count"));
            }
        }
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestFailure {
    method: String,
    path: String,
    page_origin: String,
    target_origin: String,
    kind: RequestFailureKind,
    status: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RequestFailureKind {
    Network,
    NetworkOrCors,
    Opaque,
    Http,
    Aborted,
}

async fn diagnostic(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<BootDiagnostic>,
) -> Result<Json<Value>, ApiError> {
    owner(&state, &headers)?;
    if !cfg!(debug_assertions) {
        return Err((
            StatusCode::CONFLICT,
            "Frontend diagnostics require a debug build".into(),
        ));
    }
    if let Some(chat) = &body.chat {
        let encoded = chat.to_string();
        if encoded.len() > 4096 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Chat diagnostic exceeds metadata limit".into(),
            ));
        }
        log::debug!(
            "[chat-scroll] client={} {encoded}",
            diagnostic_client(&headers)
        );
    }
    if let Some(request) = &body.request {
        log::debug!(
            "[frontend-request] client={} method={:?} path={:?} page_origin={:?} target_origin={:?} kind={:?} status={:?}",
            diagnostic_client(&headers), request.method, request.path.split(['?', '#']).next().unwrap_or(""),
            request.page_origin, request.target_origin, request.kind, request.status,
        );
    }
    let source = body.source.as_deref().filter(|source| {
        source.len() <= 100
            && source
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    });
    log::debug!("[frontend-shell/page] client={} stage={:?}, native={}, injected={}, mounted={}, error={:?}, reason={:?}, http_status={:?}, source={source:?}, line={:?}, column={:?}", diagnostic_client(&headers), body.stage, body.native, body.injected, body.mounted, body.error_name, body.reason, body.http_status, body.line, body.column);
    Ok(Json(json!({ "ok": true })))
}

fn owner(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if is_local_caller(state, headers) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Desktop lifecycle requires owner authority".into(),
        ))
    }
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    owner(&state, &headers)?;
    Ok(Json(json!({
        "generation": codeterm_service_core::startup::frontend_readiness_generation(),
        "apiVersion": codeterm_service_core::frontend_bundle_api_version::API_VERSION,
        "bundled": {
            "index": "/bundled/index.html",
            "minApiVersion": codeterm_service_core::frontend_bundle_api_version::BUNDLED_MIN_API_VERSION,
            "maxApiVersion": codeterm_service_core::frontend_bundle_api_version::BUNDLED_MAX_API_VERSION,
        },
        "listener": state.rest_api.status(),
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    generation: u64,
}

async fn ready(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Ready>,
) -> Result<Json<bool>, ApiError> {
    owner(&state, &headers)?;
    Ok(Json(codeterm_service_core::startup::mark_frontend_ready(
        body.generation,
    )))
}

async fn recovery(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<codeterm_service_core::crash::lifecycle::RecoveryState>, ApiError> {
    owner(&state, &headers)?;
    tokio::task::spawn_blocking(codeterm_service_core::crash::lifecycle::recovery_state)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn listener_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    owner(&state, &headers)?;
    let status = state.rest_api.status();
    let mut value = serde_json::to_value(&status)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    value["bindError"] = if status.diagnostic
        == Some(codeterm_service_core::startup::RestApiDiagnostic::BindOwnershipFailed)
    {
        json!(status.message)
    } else {
        Value::Null
    };
    Ok(Json(value))
}

async fn listener_restart(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    owner(&state, &headers)?;
    let accepted = state
        .rest_api
        .prepare_http_restart()
        .await
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    let status = json!({ "generation": accepted.generation, "phase": "starting", "diagnostic": null, "message": null });
    let bytes = serde_json::to_vec(&status)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((
        StatusCode::ACCEPTED,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        Body::from_stream(restart_response(
            Bytes::from(bytes),
            accepted.response_complete,
        )),
    )
        .into_response())
}

fn restart_response(
    bytes: Bytes,
    completed: tokio::sync::oneshot::Sender<()>,
) -> impl futures_core::Stream<Item = Result<Bytes, std::convert::Infallible>> {
    async_stream::stream! {
        yield Ok(bytes);
        let _ = completed.send(());
    }
}

#[cfg(test)]
mod tests;
