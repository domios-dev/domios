use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::{Arc, LazyLock},
    task::{Context, Poll},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use codeterm_util::lock_hold::TimedMutex;
use http_body::{Frame, SizeHint};
use serde::Serialize;

const RETAINED_MINUTES: u64 = 10;
const DIMENSIONS_PER_MINUTE: usize = 1024;
static CENSUS: LazyLock<Arc<RequestCensus>> = LazyLock::new(|| Arc::new(RequestCensus::default()));

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct RequestKey {
    method: String,
    route: String,
    client_id: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
struct Counts {
    requests: u64,
    request_bytes: u64,
    response_bytes: u64,
}

struct RequestCensus {
    minutes: TimedMutex<BTreeMap<u64, BTreeMap<RequestKey, Counts>>>,
}

impl Default for RequestCensus {
    fn default() -> Self {
        Self {
            minutes: TimedMutex::new("request_census", BTreeMap::new()),
        }
    }
}

#[derive(Clone, Copy)]
enum Observation {
    Request,
    RequestBytes(u64),
    ResponseBytes(u64),
}

fn current_minute() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 60
}

impl RequestCensus {
    fn observe(&self, minute: u64, key: &RequestKey, observation: Observation) {
        let mut minutes = self.minutes.lock();
        minutes.retain(|at, _| at.saturating_add(RETAINED_MINUTES) > minute);
        let dimensions = minutes.entry(minute).or_default();
        let key = if dimensions.len() >= DIMENSIONS_PER_MINUTE && !dimensions.contains_key(key) {
            RequestKey {
                method: "*".into(),
                route: "<overflow>".into(),
                client_id: "<overflow>".into(),
            }
        } else {
            key.clone()
        };
        let counts = dimensions.entry(key).or_default();
        match observation {
            Observation::Request => counts.requests += 1,
            Observation::RequestBytes(bytes) => counts.request_bytes += bytes,
            Observation::ResponseBytes(bytes) => counts.response_bytes += bytes,
        }
    }

    fn snapshot(&self) -> serde_json::Value {
        let minutes = self.minutes.lock().clone();
        let entries = minutes
            .into_iter()
            .flat_map(|(minute, dimensions)| {
                dimensions.into_iter().map(move |(key, counts)| {
                    serde_json::json!({
                        "minute_epoch": minute,
                        "method": key.method,
                        "route": key.route,
                        "client_id": key.client_id,
                        "requests": counts.requests,
                        "request_bytes": counts.request_bytes,
                        "response_bytes": counts.response_bytes,
                    })
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "retained_minutes": RETAINED_MINUTES,
            "dimensions_per_minute": DIMENSIONS_PER_MINUTE,
            "response_byte_unit": "http_body_data",
            "entries": entries,
        })
    }
}

fn client_id(request: &Request) -> String {
    request
        .headers()
        .get("x-codeterm-client-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .unwrap_or("unknown")
        .to_owned()
}

struct ObservedBody {
    inner: Body,
    key: RequestKey,
    census: Arc<RequestCensus>,
    input: bool,
}

impl HttpBody for ObservedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(frame))) = &polled {
            if let Some(data) = frame.data_ref() {
                let bytes = data.len() as u64;
                let observation = if self.input {
                    Observation::RequestBytes(bytes)
                } else {
                    Observation::ResponseBytes(bytes)
                };
                self.census
                    .observe(current_minute(), &self.key, observation);
            }
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

fn observe_body(body: Body, key: RequestKey, census: Arc<RequestCensus>, input: bool) -> Body {
    Body::new(ObservedBody {
        inner: body,
        key,
        census,
        input,
    })
}

async fn record_with(request: Request, next: Next, census: Arc<RequestCensus>) -> Response {
    let key = RequestKey {
        method: request.method().to_string(),
        route: request
            .extensions()
            .get::<MatchedPath>()
            .map(|path| path.as_str())
            .unwrap_or("<unmatched>")
            .to_owned(),
        client_id: client_id(&request),
    };
    census.observe(current_minute(), &key, Observation::Request);
    let request = request.map(|body| observe_body(body, key.clone(), census.clone(), true));
    let response = next.run(request).await;
    response.map(|body| observe_body(body, key, census, false))
}

pub(super) async fn record(request: Request, next: Next) -> Response {
    record_with(request, next, CENSUS.clone()).await
}

pub(super) fn snapshot() -> serde_json::Value {
    CENSUS.snapshot()
}

#[cfg(test)]
mod tests;
