//! `JsViewProvider` — adapts a JS plugin declaring `capabilities.view` to the
//! core [`ViewProvider`](crate::view_host::ViewProvider) trait. Every bridge
//! `ct.invoke(method, args)` from the iframe is forwarded to the plugin's
//! `viewCall(method, args)` export via the synchronous `call_json`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::vm::{call_json, PluginVm};
use crate::js_runtime::{ViewHostAction, WebhookReceiverKind};
use crate::view_host::{ViewError, ViewProvider};

pub struct JsViewProvider {
    vm: Arc<PluginVm>,
    id: String,
    webhook_receiver_kind: Option<WebhookReceiverKind>,
    host_actions: Vec<ViewHostAction>,
    media_delivery: Option<crate::media_delivery::MediaDeliveryDescriptor>,
}

impl JsViewProvider {
    pub fn with_media_delivery(
        mut self,
        descriptor: Option<crate::media_delivery::MediaDeliveryDescriptor>,
    ) -> Self {
        self.media_delivery = descriptor;
        self
    }

    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self::with_webhook_receiver(vm, id, None)
    }

    pub fn with_webhook_receiver(
        vm: Arc<PluginVm>,
        id: impl Into<String>,
        webhook_receiver_kind: Option<WebhookReceiverKind>,
    ) -> Self {
        Self {
            vm,
            id: id.into(),
            webhook_receiver_kind,
            host_actions: Vec::new(),
            media_delivery: None,
        }
    }

    /// The manifest's `viewHostActions`, the only gesture actions the host performs for this view.
    pub fn with_host_actions(mut self, host_actions: Vec<ViewHostAction>) -> Self {
        self.host_actions = host_actions;
        self
    }
}

#[async_trait]
impl ViewProvider for JsViewProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn webhook_receiver_kind(&self) -> Option<WebhookReceiverKind> {
        self.webhook_receiver_kind
    }

    fn host_actions(&self) -> &[ViewHostAction] {
        &self.host_actions
    }

    fn media_delivery(&self) -> Option<crate::media_delivery::MediaDeliveryDescriptor> {
        self.media_delivery.clone().filter(|value| value.valid())
    }

    async fn media_file(&self, id: &str) -> Result<crate::media_delivery::MediaFile, ViewError> {
        if self.media_delivery().is_none() || id.is_empty() || id.len() > 256 {
            return Err(ViewError::Call("media delivery denied".into()));
        }
        let vm = self.vm.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || call_json(&vm, "mediaFile", &[Value::String(id)]))
            .await
            .map_err(|_| ViewError::Call("media task failed".into()))?
            .map_err(|_| ViewError::Call("media file refused".into()))
    }

    async fn call(
        &self,
        method: &str,
        args: Value,
        context: &crate::view_host::ViewCallContext,
    ) -> Result<Value, ViewError> {
        // A plugin-supplied cwd must not mint a read grant; only host context can do so.
        let vm = self.vm.clone();
        let method = method.to_string();
        let invocation = context.clone();
        let plugin = self.id.clone();
        let context = serde_json::to_value(context)
            .map_err(|_| ViewError::Call("invalid view context".into()))?;
        tokio::task::spawn_blocking(move || {
            crate::view_host::with_view_call_context(&plugin, &invocation, || {
                call_json::<Value>(&vm, "viewCall", &[Value::String(method), args, context])
                    .map_err(|error| ViewError::Call(format!("{error:?}")))
            })
        })
        .await
        .map_err(|e| ViewError::Call(format!("view task failed: {e}")))?
    }
}

#[cfg(test)]
mod tests;
