use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

#[derive(Clone)]
pub enum Endpoint {
    Sse { base_url: String, cwd: String },
    Rpc(Arc<super::codex_rpc::lifecycle::Sidecar>),
}

struct RegisteredEndpoint {
    endpoint: Endpoint,
    binding_nonce: Option<String>,
}

static ENDPOINTS: LazyLock<Mutex<HashMap<String, RegisteredEndpoint>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register(tab: &str, endpoint: Endpoint) {
    ENDPOINTS.lock().insert(
        tab.into(),
        RegisteredEndpoint {
            endpoint,
            binding_nonce: None,
        },
    );
}
pub fn get(tab: &str) -> Option<Endpoint> {
    ENDPOINTS
        .lock()
        .get(tab)
        .map(|entry| entry.endpoint.clone())
}
pub fn remove(tab: &str) -> Option<Endpoint> {
    ENDPOINTS.lock().remove(tab).map(|entry| entry.endpoint)
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

pub(crate) fn bind_sse_launch(tab: &str, nonce: &str) {
    if let Some(entry) = ENDPOINTS.lock().get_mut(tab) {
        if matches!(entry.endpoint, Endpoint::Sse { .. }) {
            entry.binding_nonce = Some(nonce.into());
        }
    }
}

pub(crate) fn sse_for_nonce(tab: &str, nonce: &str) -> Option<(String, String)> {
    let endpoints = ENDPOINTS.lock();
    let entry = endpoints.get(tab)?;
    if entry.binding_nonce.as_deref() != Some(nonce) {
        return None;
    }
    match &entry.endpoint {
        Endpoint::Sse { base_url, cwd } => Some((base_url.clone(), cwd.clone())),
        _ => None,
    }
}

pub(crate) fn take_rpc() -> Vec<Arc<super::codex_rpc::lifecycle::Sidecar>> {
    let mut endpoints = ENDPOINTS.lock();
    let tabs: Vec<_> = endpoints
        .iter()
        .filter(|(_, entry)| matches!(entry.endpoint, Endpoint::Rpc(_)))
        .map(|(tab, _)| tab.clone())
        .collect();
    tabs.into_iter()
        .filter_map(|tab| match endpoints.remove(&tab)?.endpoint {
            Endpoint::Rpc(sidecar) => Some(sidecar),
            _ => None,
        })
        .collect()
}
