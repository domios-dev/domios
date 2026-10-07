use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

#[derive(Clone)]
pub enum Endpoint {
    Sse { base_url: String, cwd: String },
    Rpc(Arc<super::codex_rpc::lifecycle::Sidecar>),
}

static ENDPOINTS: LazyLock<Mutex<HashMap<String, Endpoint>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register(tab: &str, endpoint: Endpoint) {
    ENDPOINTS.lock().insert(tab.into(), endpoint);
}
pub fn get(tab: &str) -> Option<Endpoint> {
    ENDPOINTS.lock().get(tab).cloned()
}
pub fn remove(tab: &str) -> Option<Endpoint> {
    ENDPOINTS.lock().remove(tab)
}
pub fn sse(tab: &str) -> Option<(String, String)> {
    match get(tab)? {
        Endpoint::Sse { base_url, cwd } => Some((base_url, cwd)),
        _ => None,
    }
}
pub fn rpc(tab: &str) -> Option<Arc<super::codex_rpc::lifecycle::Sidecar>> {
    match get(tab)? {
        Endpoint::Rpc(sidecar) => Some(sidecar),
        _ => None,
    }
}
