use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone)]
pub enum Endpoint {
    Sse { base_url: String, cwd: String },
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
    }
}
pub(crate) fn bind_sse_launch(tab: &str, nonce: &str) {
    if let Some(entry) = ENDPOINTS.lock().get_mut(tab) {
        entry.binding_nonce = Some(nonce.into());
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
    }
}
