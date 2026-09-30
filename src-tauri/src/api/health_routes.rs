use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};

pub(crate) const REMOTE_TASK_DISPATCH_FEATURE: &str = "remote_task_dispatch";

const IDENTIFY_FEATURES: &[&str] = &[
    "multiplexer",
    "agent-detection",
    "notifications",
    "rest-api",
    "canvas",
    "delayed_recurrence",
    REMOTE_TASK_DISPATCH_FEATURE,
];

pub async fn health(AxumState(state): AxumState<AppState>) -> Json<serde_json::Value> {
    match crate::dispatch::dispatch(&crate::dispatch::Command::Health, state.svc.as_ref()) {
        Ok(r) => {
            let mut json = r.into_json();
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "version".into(),
                    serde_json::json!(env!("CARGO_PKG_VERSION")),
                );
                obj.insert("build".into(), serde_json::json!(env!("BUILD_HASH")));
                obj.insert("build_sha".into(), serde_json::json!(env!("BUILD_SHA")));
                obj.insert(
                    "instance".into(),
                    serde_json::json!({
                        "name": std::env::var("CODETERM_INSTANCE").unwrap_or_default(),
                        "api_port": std::env::var("CODETERM_API_PORT").unwrap_or_default(),
                        "vite_port": std::env::var("CODETERM_VITE_PORT").unwrap_or_default(),
                        "cdp_port": std::env::var("CODETERM_CDP_PORT").unwrap_or_default(),
                        "identifier": std::env::var("CODETERM_DEV_IDENTIFIER").unwrap_or_default(),
                    }),
                );
            }
            Json(json)
        }
        Err(_) => Json(serde_json::json!({"status": "error"})),
    }
}

pub async fn identify(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "name": "CodeTerm",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "features": IDENTIFY_FEATURES
    })))
}

#[cfg(test)]
mod tests {
    use super::IDENTIFY_FEATURES;

    #[test]
    fn identify_advertises_remote_task_dispatch_capability() {
        assert!(IDENTIFY_FEATURES.contains(&super::REMOTE_TASK_DISPATCH_FEATURE));
    }

    #[test]
    fn identify_advertises_delayed_recurrence_capability() {
        assert!(IDENTIFY_FEATURES.contains(&"delayed_recurrence"));
    }
}
