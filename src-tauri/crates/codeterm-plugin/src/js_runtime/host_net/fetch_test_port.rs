//! Answers `host.fetch` for one exact host in tests. It stands where the network would, after the
//! real request is built, so header validation still runs and no test reaches a real endpoint.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;

pub struct ObservedRequest {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
}

pub struct FetchReply {
    pub status: u16,
    pub body: String,
}

type Port = Arc<dyn Fn(&ObservedRequest) -> FetchReply + Send + Sync>;

static PORTS: LazyLock<Mutex<HashMap<String, Port>>> = LazyLock::new(Default::default);

/// Removes the port when dropped, so one test's fake never answers another's request.
pub struct InstalledPort(String);

impl Drop for InstalledPort {
    fn drop(&mut self) {
        PORTS.lock().remove(&self.0);
    }
}

pub fn install(
    host: &str,
    port: impl Fn(&ObservedRequest) -> FetchReply + Send + Sync + 'static,
) -> InstalledPort {
    PORTS.lock().insert(host.to_string(), Arc::new(port));
    InstalledPort(host.to_string())
}

pub(super) fn answer(request: &reqwest::Request) -> Option<String> {
    let port = PORTS.lock().get(request.url().host_str()?).cloned()?;
    let reply = port(&ObservedRequest {
        url: request.url().to_string(),
        method: request.method().to_string(),
        headers: request
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect(),
    });
    Some(serde_json::json!({"status": reply.status, "headers": {}, "body": reply.body}).to_string())
}
