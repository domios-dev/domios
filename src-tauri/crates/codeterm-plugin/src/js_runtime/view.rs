//! `JsViewProvider` — adapts a JS plugin declaring `capabilities.view` to the
//! core [`ViewProvider`](crate::view_host::ViewProvider) trait. Every bridge
//! `ct.invoke(method, args)` from the iframe is forwarded to the plugin's
//! `viewCall(method, args)` export via the synchronous `call_json`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::vm::{call_json, PluginVm};
use crate::js_runtime::WebhookReceiverKind;
use crate::view_host::{ViewError, ViewProvider};

pub struct JsViewProvider {
    vm: Arc<PluginVm>,
    id: String,
    webhook_receiver_kind: Option<WebhookReceiverKind>,
}

impl JsViewProvider {
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
        }
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

    async fn call(&self, method: &str, args: Value) -> Result<Value, ViewError> {
        // A plugin-supplied cwd must not mint a read grant; only host context can do so.
        let vm = self.vm.clone();
        let method = method.to_string();
        tokio::task::spawn_blocking(move || {
            call_json::<Value>(&vm, "viewCall", &[Value::String(method), args])
        })
        .await
        .map_err(|e| ViewError::Call(format!("view task failed: {e}")))?
        .map_err(|e| ViewError::Call(format!("{e:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-view", src.to_string()).expect("vm"))
    }

    #[tokio::test]
    async fn routes_bridge_calls_to_view_call() {
        let vm = vm_with(
            r#"module.exports.default = {
                viewCall(method, args) {
                    if (method === "echo") return { got: args.x * 2 };
                    return { error: "unknown" };
                }
            }"#,
        );
        let v = JsViewProvider::new(vm, "test-view");
        let out = v
            .call("echo", serde_json::json!({ "x": 21 }))
            .await
            .unwrap();
        assert_eq!(out["got"], 42);
    }

    /// NEGATIVE CONTROL for the grant seam: view-call args are iframe-authored
    /// (the host stamps cwd only when the iframe omits it), so an `args.cwd`
    /// must never mint a read grant — otherwise any drop-in with a view could
    /// self-issue read access to arbitrary paths. Reads under a tab cwd work
    /// only after a host-driven bubble/glance call granted that root.
    #[tokio::test]
    async fn view_args_cwd_never_mints_a_read_grant() {
        use crate::js_runtime::host_api::PluginSandbox;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("loot")).unwrap();
        let sandbox = Arc::new(PluginSandbox::restricted(vec![], vec![]));
        let vm = Arc::new(
            PluginVm::from_source_sandboxed(
                "confined-view",
                r#"module.exports.default = {
                    viewCall(method, args) { return { names: host.listChildDirs(args.cwd) }; }
                }"#
                .to_string(),
                None,
                sandbox.clone(),
            )
            .expect("vm"),
        );
        let v = JsViewProvider::new(vm, "confined-view");
        let cwd = tmp.path().to_string_lossy().into_owned();

        let out = v
            .call("probe", serde_json::json!({ "cwd": cwd }))
            .await
            .unwrap();
        assert_eq!(
            out["names"],
            serde_json::Value::Null,
            "an iframe-supplied cwd must stay denied"
        );

        // Once the HOST granted the same cwd (bubble/glance path), the view works.
        sandbox.grant_cwd_read_root(&cwd);
        let out = v
            .call("probe", serde_json::json!({ "cwd": cwd }))
            .await
            .unwrap();
        assert_eq!(out["names"], serde_json::json!(["loot"]));
    }
}
