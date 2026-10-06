use std::sync::Arc;

use serde::Deserialize;

use super::vm::{call_json, PluginVm};
use crate::event_source::EventSourceProvider;
use crate::js_runtime::manifest::EventSourceDecl;

pub struct JsEventSource {
    vm: Arc<PluginVm>,
    declarations: Vec<EventSourceDecl>,
}

impl JsEventSource {
    pub fn new(vm: Arc<PluginVm>, declarations: Vec<EventSourceDecl>) -> Self {
        Self { vm, declarations }
    }
}

#[derive(Deserialize)]
struct EventSourceResult {
    #[serde(default)]
    error: Option<String>,
}

impl EventSourceProvider for JsEventSource {
    fn declarations(&self) -> &[EventSourceDecl] {
        &self.declarations
    }

    fn subscribe(
        &self,
        kind: &str,
        params: &serde_json::Value,
        subscription_id: &str,
        token: &str,
    ) -> Result<(), String> {
        self.call_export(
            "subscribe",
            serde_json::json!({
                "kind": kind,
                "params": params,
                "subscriptionId": subscription_id,
                "token": token,
            }),
        )
    }

    fn unsubscribe(&self, kind: &str, subscription_id: &str) -> Result<(), String> {
        self.call_export(
            "unsubscribe",
            serde_json::json!({ "kind": kind, "subscriptionId": subscription_id }),
        )
    }
}

impl JsEventSource {
    fn call_export(&self, name: &str, payload: serde_json::Value) -> Result<(), String> {
        let result = call_json::<EventSourceResult>(&self.vm, name, &[payload])
            .map_err(|error| format!("plugin {name} failed: {error:?}"))?;
        match result.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
