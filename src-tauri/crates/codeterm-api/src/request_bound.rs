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

static ACTIVE: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static RELEASED: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub(crate) struct RequestSlot(String);

impl RequestSlot {
    pub(crate) async fn acquire_when_available(key: String) -> Result<Self, RequestError> {
        tokio::time::timeout(PLUGIN_DEADLINE, async {
            loop {
                let released = RELEASED.notified();
                tokio::pin!(released);
                released.as_mut().enable();
                if let Ok(slot) = Self::acquire(key.clone()) {
                    return slot;
                }
                released.await;
            }
        })
        .await
        .map_err(|_| {
            unavailable(
                "request_busy",
                "Chat snapshot is still running. Retry when it finishes.",
            )
        })
    }

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
        RELEASED.notify_waiters();
    }
}

#[cfg(test)]
mod tests;
