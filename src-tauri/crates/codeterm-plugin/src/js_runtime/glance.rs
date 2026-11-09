//! `JsGlanceProvider` — adapts a JS plugin declaring `capabilities.glanceView`
//! to the core [`GlanceProvider`](crate::glance::GlanceProvider) trait. Mirrors
//! `JsTranscriber`/`JsSecretBackend`: wraps the shared `Arc<PluginVm>` and
//! dispatches to exported plugin functions via the synchronous `call_json`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::vm::{call_json, has_method, PluginVm};
use crate::glance::{GlanceError, GlanceProvider};
use crate::view::GlanceView;

pub struct JsGlanceProvider {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsGlanceProvider {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[async_trait]
impl GlanceProvider for JsGlanceProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn render(&self, ctx: Value) -> Result<GlanceView, GlanceError> {
        // async runtime (starves all IPC) — hop to the blocking pool; the cwd
        // grant contends on the same VM lock, so it hops too.
        let vm = self.vm.clone();
        crate::js_runtime::vm::spawn_blocking(move || {
            super::vm::grant_ctx_cwd(&vm, &ctx);
            call_json::<GlanceView>(&vm, "renderGlance", &[ctx])
        })
        .await
        .map_err(|e| GlanceError::Render(format!("glance render task failed: {e}")))?
        .map_err(|e| GlanceError::Render(format!("{e:?}")))
    }

    async fn action(&self, action: &str, ctx: Value) -> Result<GlanceView, GlanceError> {
        let vm = self.vm.clone();
        let action = action.to_string();
        crate::js_runtime::vm::spawn_blocking(move || {
            super::vm::grant_ctx_cwd(&vm, &ctx);
            // `glanceAction` is optional — without it, a button just re-renders.
            if has_method(&vm, "glanceAction") {
                call_json::<GlanceView>(&vm, "glanceAction", &[Value::String(action), ctx])
            } else {
                call_json::<GlanceView>(&vm, "renderGlance", &[ctx])
            }
        })
        .await
        .map_err(|e| GlanceError::Render(format!("glance action task failed: {e}")))?
        .map_err(|e| GlanceError::Render(format!("{e:?}")))
    }
}

#[cfg(test)]
mod tests;
