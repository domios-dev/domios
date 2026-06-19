//! `JsViewProvider` — adapts a JS plugin declaring `capabilities.view` to the
//! core [`ViewProvider`](crate::view_host::ViewProvider) trait. Every bridge
//! `ct.invoke(method, args)` from the iframe is forwarded to the plugin's single
//! `viewCall(method, args)` export via the synchronous `call_json`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::vm::{call_json, PluginVm};
use crate::view_host::{ViewError, ViewProvider};

pub struct JsViewProvider {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsViewProvider {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[async_trait]
impl ViewProvider for JsViewProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn call(&self, method: &str, args: Value) -> Result<Value, ViewError> {
        // VM lock + JS execution are synchronous — keep them off the async runtime.
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
}
