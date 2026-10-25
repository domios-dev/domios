use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};
use codeterm_service_core::ports::socket::REMOTE_TASK_DISPATCH_FEATURE;

#[derive(Clone)]
pub struct NativeDiagnosticsPort(pub std::sync::Arc<dyn Fn() -> Option<u16> + Send + Sync>);

#[derive(Clone)]
pub struct NativeFrontendIdentity {
    pub port: std::sync::Arc<dyn Fn() -> Option<u16> + Send + Sync>,
    pub identifier: String,
}

impl NativeFrontendIdentity {
    pub fn vite_port(&self) -> String {
        if cfg!(debug_assertions) {
            (self.port)()
                .map(|port| port.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        }
    }

    pub fn identifier(&self) -> &str {
        if cfg!(debug_assertions) {
            &self.identifier
        } else {
            ""
        }
    }
}

impl NativeDiagnosticsPort {
    pub fn cdp_port(&self) -> String {
        self.for_build(cfg!(debug_assertions))
    }

    fn for_build(&self, development: bool) -> String {
        if development {
            (self.0)()
                .filter(|port| *port >= 1024)
                .map(|port| port.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        }
    }
}

const IDENTIFY_FEATURES: &[&str] = &[
    "multiplexer",
    "agent-detection",
    "notifications",
    "rest-api",
    "canvas",
    "delayed_recurrence",
    REMOTE_TASK_DISPATCH_FEATURE,
];

/// Liveness must never wait on the mesh lock, so a contended read reports no name.
fn local_display_name(state: &AppState) -> String {
    state
        .mesh
        .try_lock()
        .map(|mesh| mesh.ident.instance_name.clone())
        .unwrap_or_default()
}

pub async fn health(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    match codeterm_service_core::dispatch::health_result() {
        Ok(r) => {
            let mut json = r.into_json();
            if let Some(obj) = json.as_object_mut() {
                obj.insert("startup".into(), serde_json::json!({ "state": "ready" }));
                obj.insert(
                    "version".into(),
                    serde_json::json!(codeterm_build_info::version()),
                );
                obj.insert(
                    "build".into(),
                    serde_json::json!(codeterm_build_info::hash()),
                );
                obj.insert(
                    "build_sha".into(),
                    serde_json::json!(codeterm_build_info::sha()),
                );
                obj.insert(
                    "boot_id".into(),
                    serde_json::json!(state.svc.events.boot_id()),
                );
                obj.insert(
                    "apiVersion".into(),
                    serde_json::json!(
                        codeterm_service_core::frontend_bundle_api_version::API_VERSION
                    ),
                );
                obj.insert(
                    "instance".into(),
                    serde_json::json!({
                        "name": std::env::var("CODETERM_INSTANCE").unwrap_or_default(),
                        "display_name": local_display_name(&state),
                        "api_port": std::env::var("CODETERM_API_PORT").unwrap_or_default(),
                        "vite_port": state.svc.host_extension::<NativeFrontendIdentity>().map(|identity| identity.vite_port()).unwrap_or_default(),
                        "cdp_port": state.svc.host_extension::<NativeDiagnosticsPort>().map(|port| port.cdp_port()).unwrap_or_default(),
                        "identifier": state.svc.host_extension::<NativeFrontendIdentity>().map(|identity| identity.identifier().to_owned()).unwrap_or_default(),
                    }),
                );
            }
            Json(json)
        }
        Err(_) => Json(serde_json::json!({
            "status": "error",
            "apiVersion": codeterm_service_core::frontend_bundle_api_version::API_VERSION,
        })),
    }
}

pub async fn identify(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "name": codeterm_build_info::PRODUCT_NAME,
        "version": codeterm_build_info::version(),
        "platform": std::env::consts::OS,
        "features": IDENTIFY_FEATURES
    })))
}

#[cfg(test)]
mod tests;
