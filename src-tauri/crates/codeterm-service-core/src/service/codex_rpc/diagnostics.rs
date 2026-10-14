use serde::Serialize;
use std::sync::atomic::Ordering;

#[derive(Debug, Serialize)]
pub struct Diagnostics {
    pub port: u16,
    pub binary_path: String,
    pub protocol_version: String,
    pub thread_id: String,
    pub selection_verified: bool,
    pub active_turn: bool,
    pub transcript_source: &'static str,
    pub jsonl_tailer: bool,
    pub auth: super::auth::AuthState,
}

pub fn for_tab(tab: &str) -> Option<Diagnostics> {
    let sidecar = super::lifecycle::for_tab(tab)?;
    let thread_id = sidecar.thread.lock().clone();
    let auth = sidecar.auth.lock().clone();
    Some(Diagnostics {
        port: sidecar.port,
        binary_path: sidecar.binary.display().to_string(),
        protocol_version: sidecar.version.clone(),
        thread_id,
        selection_verified: sidecar.verified.load(Ordering::Acquire),
        active_turn: sidecar.busy.load(Ordering::Acquire),
        transcript_source: "app-server",
        jsonl_tailer: false,
        auth,
    })
}

pub fn fallback(svc: &crate::service::ServiceContext, tab: &str, provider: &str, reason: &str) {
    log::warn!("[launch] tab={tab} structured_control=plain-pty reason={reason}");
    svc.notify_model_resolution_warning(
        tab,
        provider,
        &format!("Structured control unavailable; using the plain terminal: {reason}"),
    );
}
