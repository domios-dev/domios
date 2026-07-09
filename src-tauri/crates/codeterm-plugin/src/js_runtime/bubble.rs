//! `JsBubbleProvider` — adapts a JS plugin declaring `capabilities.statusBubble`
//! to the core [`BubbleProvider`](crate::bubble::BubbleProvider) trait. Mirrors
//! `JsGlanceProvider`: wraps the shared `Arc<PluginVm>` and dispatches via the
//! synchronous `call_json`. The plugin returns the pill object, or `null` to hide.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::vm::{call_json, PluginVm};
use crate::bubble::{BubbleData, BubbleError, BubbleProvider};

pub struct JsBubbleProvider {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsBubbleProvider {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[async_trait]
impl BubbleProvider for JsBubbleProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn bubble(&self, ctx: Value) -> Result<Option<BubbleData>, BubbleError> {
        // VM lock + JS execution are synchronous — keep them off the async runtime.
        let vm = self.vm.clone();
        tokio::task::spawn_blocking(move || {
            call_json::<Option<BubbleData>>(&vm, "statusBubble", &[ctx])
        })
        .await
        .map_err(|e| BubbleError::Render(format!("bubble task failed: {e}")))?
        .map_err(|e| BubbleError::Render(format!("{e:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Tone;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-bubble", src.to_string()).expect("vm"))
    }

    #[tokio::test]
    async fn returns_pill() {
        let vm = vm_with(
            r#"module.exports.default = { statusBubble() { return { label: "main", tone: "ok" }; } }"#,
        );
        let b = JsBubbleProvider::new(vm, "test-bubble");
        let pill = b.bubble(serde_json::json!({})).await.unwrap().unwrap();
        assert_eq!(pill.label, "main");
        assert_eq!(pill.tone, Tone::Ok);
    }

    #[tokio::test]
    async fn null_hides_the_pill() {
        let vm = vm_with(r#"module.exports.default = { statusBubble() { return null; } }"#);
        let b = JsBubbleProvider::new(vm, "test-bubble");
        assert!(b.bubble(serde_json::json!({})).await.unwrap().is_none());
    }
}
