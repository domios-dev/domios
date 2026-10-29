use crate::{auth::CallerIdentity, routes::AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use codeterm_plugin::view_host::{ViewCallContext, ViewProvider, ViewRegistryHandle};
use codeterm_service_core::service::ServiceContext;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

pub(super) struct DocumentLease {
    plugin: String,
    lease: String,
    registration: String,
    provider: Arc<dyn ViewProvider>,
    pub context: ViewCallContext,
}
impl DocumentLease {
    pub fn capture(
        service: &ServiceContext,
        plugin: &str,
        headers: &HeaderMap,
    ) -> Result<Self, ()> {
        let lease = headers
            .get("x-domios-plugin-view-frame")
            .and_then(|value| value.to_str().ok())
            .ok_or(())?
            .to_string();
        let registration = headers
            .get("x-domios-plugin-media-frame")
            .and_then(|value| value.to_str().ok())
            .ok_or(())?
            .to_string();
        let (provider, context) =
            codeterm_plugin::media_delivery::view_frame(plugin, &lease).map_err(|_| ())?;
        let found = Self {
            plugin: plugin.into(),
            lease,
            registration,
            provider,
            context,
        };
        found.validate(service)?;
        Ok(found)
    }
    pub fn validate(&self, service: &ServiceContext) -> Result<(), ()> {
        let (provider, context) =
            codeterm_plugin::media_delivery::view_frame(&self.plugin, &self.lease)
                .map_err(|_| ())?;
        let current = service
            .ext::<ViewRegistryHandle>()
            .get(&self.plugin)
            .ok_or(())?;
        if !Arc::ptr_eq(&provider, &self.provider)
            || !Arc::ptr_eq(&current, &self.provider)
            || context != self.context
            || codeterm_plugin::media_delivery::frame_context(&self.plugin, &self.registration)
                .map_err(|_| ())?
                != context
            || service.validate_plugin_view_context(&context).is_err()
        {
            return Err(());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    plugin_id: String,
    reference: String,
}

pub async fn resolve(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    if !matches!(
        caller.as_ref().map(|caller| &caller.0),
        Some(CallerIdentity::Local)
    ) {
        return Err((StatusCode::FORBIDDEN, "local owner required"));
    }
    tokio::task::spawn_blocking(move || {
        let stale = || Json(serde_json::json!({"ok":false,"code":"denied","reason":"stale"}));
        let Ok(document) = DocumentLease::capture(&state.svc, &request.plugin_id, &headers) else {
            return stale();
        };
        let result = codeterm_sentinel_host::resolve_plugin_run_view(
            &state.svc,
            &document.context,
            &request.reference,
        );
        if document.validate(&state.svc).is_err() {
            return stale();
        }
        Json(
            result
                .unwrap_or_else(|_| serde_json::json!({"ok":false,"code":"denied","reason":"run"})),
        )
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "run view unavailable"))
}

#[cfg(test)]
mod tests;
