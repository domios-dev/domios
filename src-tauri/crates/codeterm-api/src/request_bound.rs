use axum::{http::StatusCode, Json};
use parking_lot::Mutex;
use std::{collections::HashSet, time::Duration};

pub(crate) const PLUGIN_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) type RequestError = (StatusCode, Json<serde_json::Value>);

pub(crate) fn unavailable(code: &str, message: impl ToString) -> RequestError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "ok": false, "code": code, "message": message.to_string()
        })),
    )
}

pub(crate) async fn bounded<T>(
    deadline: Duration,
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, RequestError> {
    tokio::time::timeout(deadline, work)
        .await
        .map_err(|_| unavailable("plugin_unavailable", "Plugin request exceeded its deadline"))?
        .map_err(|error| unavailable("plugin_unavailable", error))
}

pub(crate) async fn blocking<T: Send + 'static>(
    key: String,
    work: impl FnOnce(RequestSlot) -> Result<T, String> + Send + 'static,
) -> Result<T, RequestError> {
    let slot = RequestSlot::acquire(key)?;
    let worker = tokio::task::spawn_blocking(move || work(slot));
    bounded(PLUGIN_DEADLINE, async {
        worker.await.map_err(|error| error.to_string())?
    })
    .await
}

static ACTIVE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub(crate) struct RequestSlot(String);

impl RequestSlot {
    pub(crate) fn acquire(key: String) -> Result<Self, RequestError> {
        let mut active = ACTIVE.lock();
        if !active.get_or_insert_default().insert(key.clone()) {
            return Err(unavailable("request_busy", "Request already running"));
        }
        Ok(Self(key))
    }
}

impl Drop for RequestSlot {
    fn drop(&mut self) {
        if let Some(active) = ACTIVE.lock().as_mut() {
            active.remove(&self.0);
        }
    }
}

#[cfg(test)]
mod tests;
