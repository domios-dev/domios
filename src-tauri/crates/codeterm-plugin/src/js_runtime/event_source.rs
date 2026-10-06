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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_source_js_mapping_passes_subscription_identity_and_handles_refusal() {
        let vm = Arc::new(
            PluginVm::from_source(
                "fixture",
                r#"module.exports.default = {
                    subscribe(value) {
                        return value.kind === "fixture.message" && value.params.room === "one"
                            && value.subscriptionId === "sub" && value.token === "secret"
                            ? {} : { error: "subscription mapping mismatch" };
                    },
                    unsubscribe(value) {
                        return value.kind === "fixture.message" && value.subscriptionId === "sub"
                            ? {} : { error: "unsubscribe mapping mismatch" };
                    }
                };"#
                .into(),
            )
            .unwrap(),
        );
        let provider = JsEventSource::new(
            vm,
            vec![EventSourceDecl {
                kind: "fixture.message".into(),
                title: "Fixture message".into(),
                params: None,
                payload: None,
            }],
        );

        provider
            .subscribe(
                "fixture.message",
                &serde_json::json!({"room":"one"}),
                "sub",
                "secret",
            )
            .unwrap();
        provider.unsubscribe("fixture.message", "sub").unwrap();
        assert!(provider
            .subscribe("fixture.message", &serde_json::json!({}), "bad", "secret")
            .is_err());
    }
}

#[derive(Deserialize)]
struct EventSourceResult {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    admitted: Option<bool>,
    #[serde(default)]
    reason: Option<String>,
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
        if let Some(error) = result.error {
            return Err(error);
        }
        if result.admitted == Some(false) {
            return Err(result
                .reason
                .unwrap_or_else(|| "event admission refused".into()));
        }
        Ok(())
    }
}
