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
        call_json::<GlanceView>(&self.vm, "renderGlance", &[ctx])
            .map_err(|e| GlanceError::Render(format!("{e:?}")))
    }

    async fn action(&self, action: &str, ctx: Value) -> Result<GlanceView, GlanceError> {
        // `glanceAction` is optional — without it, a button just re-renders.
        if !has_method(&self.vm, "glanceAction") {
            return self.render(ctx).await;
        }
        call_json::<GlanceView>(
            &self.vm,
            "glanceAction",
            &[Value::String(action.to_string()), ctx],
        )
        .map_err(|e| GlanceError::Render(format!("{e:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ViewNode;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-glance", src.to_string()).expect("vm"))
    }

    #[tokio::test]
    async fn render_returns_view_tree() {
        let vm = vm_with(
            r#"module.exports.default = {
                renderGlance() { return { title: "T", nodes: [
                    { kind: "badge", label: "Up", tone: "ok" },
                    { kind: "button", label: "Settings", action: "settings" }
                ] }; }
            }"#,
        );
        let g = JsGlanceProvider::new(vm, "test-glance");
        let v = g.render(serde_json::json!({})).await.unwrap();
        assert_eq!(v.title.as_deref(), Some("T"));
        assert_eq!(v.nodes.len(), 2);
        assert!(matches!(&v.nodes[0], ViewNode::Badge { label, .. } if label == "Up"));
        assert!(matches!(&v.nodes[1], ViewNode::Button { action, .. } if action == "settings"));
    }

    #[tokio::test]
    async fn action_falls_back_to_render_when_method_absent() {
        let vm = vm_with(
            r#"module.exports.default = { renderGlance() { return { nodes: [ { kind: "text", text: "x" } ] }; } }"#,
        );
        let g = JsGlanceProvider::new(vm, "test-glance");
        let v = g.action("whatever", serde_json::json!({})).await.unwrap();
        assert_eq!(v.nodes.len(), 1);
    }
}
