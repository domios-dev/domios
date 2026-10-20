use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::MethodRouter,
    Extension, Json,
};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::routes::AppState;

const WINDOW: Duration = Duration::from_secs(60);
const REQUESTS_PER_WINDOW: usize = 120;
const MAX_BATCH: usize = 128;
// A burst of previews in one reply queues for a permit; the deadline only bounds a stuck read.
const PERMIT_DEADLINE: Duration = Duration::from_secs(10);

pub(super) struct Admission {
    window: Mutex<(Instant, usize)>,
    active: Arc<Semaphore>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl Default for Admission {
    fn default() -> Self {
        Self::with_clock(Arc::new(Instant::now))
    }
}

impl Admission {
    pub(super) fn with_clock(clock: Arc<dyn Fn() -> Instant + Send + Sync>) -> Self {
        Self {
            window: Mutex::new((clock(), 0)),
            active: Arc::new(Semaphore::new(4)),
            clock,
        }
    }

    fn count(&self, now: Instant) -> Result<(), StatusCode> {
        let mut window = self.window.lock();
        if now.saturating_duration_since(window.0) >= WINDOW {
            *window = (now, 0);
        }
        if window.1 >= REQUESTS_PER_WINDOW {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        window.1 += 1;
        Ok(())
    }

    async fn admit_within(
        &self,
        now: Instant,
        deadline: Duration,
    ) -> Result<OwnedSemaphorePermit, StatusCode> {
        self.count(now)?;
        tokio::time::timeout(deadline, self.active.clone().acquire_owned())
            .await
            .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?
            .map_err(|_| StatusCode::TOO_MANY_REQUESTS)
    }

    async fn admit(&self, now: Instant) -> Result<OwnedSemaphorePermit, StatusCode> {
        self.admit_within(now, PERMIT_DEADLINE).await
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadBody {
    path: String,
    kind: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckBody {
    paths: Vec<String>,
}

pub(super) fn admission() -> Arc<Admission> {
    Arc::new(Admission::default())
}

pub(super) fn bounded(
    route: MethodRouter<AppState>,
    admission: &Arc<Admission>,
) -> MethodRouter<AppState> {
    route
        .layer::<_, std::convert::Infallible>(DefaultBodyLimit::max(64 * 1024))
        .layer::<_, std::convert::Infallible>(Extension(admission.clone()))
}

fn refusal() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Preview unavailable"})),
    )
        .into_response()
}

pub(super) async fn read(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Extension(admission): Extension<Arc<Admission>>,
    Json(body): Json<ReadBody>,
) -> Response {
    let permit = match admission.admit((admission.clock)()).await {
        Ok(permit) => permit,
        Err(status) => return status.into_response(),
    };
    let configured = state.config.lock().api.preview_secret_paths.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        codeterm_service_core::service::local_preview::read(
            &state.svc,
            &tab,
            &configured,
            &body.path,
            &body.kind,
        )
    })
    .await;
    match result {
        Ok(Ok(file)) => Json(file).into_response(),
        _ => {
            log::debug!("[local-preview] read refused");
            refusal()
        }
    }
}

pub(super) async fn check(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Extension(admission): Extension<Arc<Admission>>,
    Json(body): Json<CheckBody>,
) -> Response {
    if body.paths.len() > MAX_BATCH {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let permit = match admission.admit((admission.clock)()).await {
        Ok(permit) => permit,
        Err(status) => return status.into_response(),
    };
    let configured = state.config.lock().api.preview_secret_paths.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        codeterm_service_core::service::local_preview::check(
            &state.svc,
            &tab,
            &configured,
            body.paths,
        )
    })
    .await;
    match result {
        Ok(Ok(files)) => Json(files).into_response(),
        _ => {
            log::debug!("[local-preview] batch refused");
            refusal()
        }
    }
}

pub(super) async fn secure_headers(req: Request, next: Next) -> Response {
    let scoped = codeterm_service_core::api_support::access_policy::is_preview_read(
        req.method(),
        req.uri().path(),
    );
    let mut response = next.run(req).await;
    if scoped {
        let headers = response.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; sandbox; frame-ancestors 'none'"),
        );
    }
    response
}

#[cfg(test)]
mod tests;
