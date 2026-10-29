//! `JsViewProvider` — adapts a JS plugin declaring `capabilities.view` to the
//! core [`ViewProvider`](crate::view_host::ViewProvider) trait. Every bridge
//! `ct.invoke(method, args)` from the iframe is forwarded to the plugin's
//! `viewCall(method, args)` export via the synchronous `call_json`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::vm::{call_json, PluginVm};
mod media_lookup;
use crate::js_runtime::{ViewHostAction, WebhookReceiverKind};
use crate::view_host::{ViewError, ViewProvider};

pub struct JsViewProvider {
    vm: Arc<PluginVm>,
    id: String,
    webhook_receiver_kind: Option<WebhookReceiverKind>,
    host_actions: Vec<ViewHostAction>,
    view_inputs: Option<crate::media_delivery::ViewInputDescriptor>,
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
            view_inputs: None,
            media_delivery: None,
        }
    }

    /// The manifest's `viewHostActions`, the only gesture actions the host performs for this view.
    pub fn with_host_actions(mut self, host_actions: Vec<ViewHostAction>) -> Self {
        self.host_actions = host_actions;
        self
    }

    /// The manifest's `viewInputs`: what `pickFile` may ingest, as media delivery's descriptor.
    pub fn with_view_inputs(mut self, inputs: Option<&crate::js_runtime::ViewInputs>) -> Self {
        self.view_inputs = inputs.map(|inputs| crate::media_delivery::ViewInputDescriptor {
            accept: inputs
                .accept
                .iter()
                .map(|kind| kind.as_str().to_owned())
                .collect(),
            max_bytes: inputs.max_bytes,
            max_files: inputs.max_files,
        });
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

    fn view_inputs(&self) -> Option<crate::media_delivery::ViewInputDescriptor> {
        self.view_inputs.clone().filter(|inputs| inputs.valid())
    }

    fn media_delivery(&self) -> Option<crate::media_delivery::MediaDeliveryDescriptor> {
        self.media_delivery.clone().filter(|value| value.valid())
    }

    async fn media_file(
        &self,
        id: &str,
        context: &crate::view_host::ViewCallContext,
    ) -> Result<crate::media_delivery::MediaFile, ViewError> {
        if self.media_delivery().is_none() || id.is_empty() || id.len() > 256 {
            return Err(ViewError::Call("media delivery denied".into()));
        }
        media_lookup::lookup(
            self.vm.clone(),
            self.id.clone(),
            id.to_owned(),
            context.clone(),
            crate::converter::ConversionCancellation::default(),
            media_lookup::deadline(),
        )
        .await
    }

    async fn run_media_file(
        &self,
        id: &str,
        context: &crate::media_delivery::RunMediaLookupContext,
    ) -> Result<crate::media_delivery::MediaFile, ViewError> {
        if self.media_delivery().is_none()
            || id.is_empty()
            || id.len() > 256
            || !context.valid_for(&self.id)
        {
            return Err(ViewError::Call("durable media lookup denied".into()));
        }
        media_lookup::lookup_run(
            self.vm.clone(),
            self.id.clone(),
            id.into(),
            context.clone(),
            crate::converter::ConversionCancellation::default(),
            media_lookup::deadline(),
        )
        .await
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
