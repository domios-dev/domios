//! `codeterm plugin receiver-target` — the host-owned tab that receives a webhook plugin's events.

use serde_json::{json, Value};

use crate::client::RpcClient;

fn request(plugin_id: &str, set: Option<&str>, clear: bool) -> (&'static str, Value) {
    match (set, clear) {
        (Some(tab_id), _) => (
            "plugin.receiver_target.set",
            json!({ "plugin_id": plugin_id, "tab_id": tab_id }),
        ),
        (None, true) => (
            "plugin.receiver_target.clear",
            json!({ "plugin_id": plugin_id }),
        ),
        (None, false) => (
            "plugin.receiver_target.get",
            json!({ "plugin_id": plugin_id }),
        ),
    }
}

fn run(
    client: &mut dyn RpcClient,
    plugin_id: &str,
    set: Option<&str>,
    clear: bool,
) -> Result<Value, String> {
    let (method, params) = request(plugin_id, set, clear);
    client
        .call_detailed(method, Some(params))
        .map_err(|error| error.to_string())
}

pub(super) fn run_over_socket(
    plugin_id: &str,
    set: Option<&str>,
    clear: bool,
) -> Result<Value, String> {
    let (mut client, _) = crate::client::SocketClient::connect_local()?;
    run(&mut client, plugin_id, set, clear)
}

/// One tab-separated line: plugin id, bound tab (or the General Agent fallback), liveness.
pub(super) fn render(value: &Value) -> Option<String> {
    let plugin_id = value.get("pluginId")?.as_str()?;
    let live = value.get("live").and_then(Value::as_bool).unwrap_or(false);
    Some(match value.get("tabId").and_then(Value::as_str) {
        Some(tab_id) if live => format!("{plugin_id}\t{tab_id}\tlive"),
        Some(tab_id) => format!("{plugin_id}\t{tab_id}\tnot live (General Agent receives)"),
        None => format!("{plugin_id}\tunbound (General Agent receives)"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{RpcError, SocketCallError};

    struct FakeRpc {
        calls: Vec<(String, Option<Value>)>,
        refusal: Option<RpcError>,
    }

    impl FakeRpc {
        fn answering() -> Self {
            Self {
                calls: Vec::new(),
                refusal: None,
            }
        }
    }

    impl RpcClient for FakeRpc {
        fn call_detailed(
            &mut self,
            method: &str,
            params: Option<Value>,
        ) -> Result<Value, SocketCallError> {
            self.calls.push((method.to_string(), params));
            match self.refusal.take() {
                Some(error) => Err(SocketCallError::Rpc(error)),
                None => Ok(json!({ "pluginId": "receiver", "tabId": null, "live": false })),
            }
        }
    }

    #[test]
    fn pebble_receiver_target_cli_sends_get_set_and_clear() {
        let cases = [
            (
                None,
                false,
                "plugin.receiver_target.get",
                json!({ "plugin_id": "receiver" }),
            ),
            (
                Some("tab-1"),
                false,
                "plugin.receiver_target.set",
                json!({ "plugin_id": "receiver", "tab_id": "tab-1" }),
            ),
            (
                None,
                true,
                "plugin.receiver_target.clear",
                json!({ "plugin_id": "receiver" }),
            ),
        ];
        for (set, clear, method, params) in cases {
            let mut rpc = FakeRpc::answering();
            run(&mut rpc, "receiver", set, clear).expect("daemon answer");
            assert_eq!(rpc.calls, vec![(method.to_string(), Some(params))]);
        }
    }

    #[test]
    fn pebble_receiver_target_cli_surfaces_daemon_refusal() {
        let refusal = RpcError {
            code: -32602,
            message: "tab refused".into(),
            data: None,
        };
        let expected = SocketCallError::Rpc(RpcError {
            code: refusal.code,
            message: refusal.message.clone(),
            data: None,
        })
        .to_string();
        let mut rpc = FakeRpc {
            calls: Vec::new(),
            refusal: Some(refusal),
        };
        assert_eq!(
            run(&mut rpc, "receiver", Some("tab-on-a-peer"), false),
            Err(expected)
        );
    }

    #[test]
    fn pebble_receiver_target_cli_renders_binding_fields() {
        let bound = render(&json!({ "pluginId": "receiver", "tabId": "tab-1", "live": true }))
            .expect("view line");
        assert_eq!(
            bound.split('\t').collect::<Vec<_>>(),
            vec!["receiver", "tab-1", "live"]
        );
        let unbound = render(&json!({ "pluginId": "receiver", "tabId": null, "live": false }))
            .expect("view line");
        assert_eq!(unbound.split('\t').next(), Some("receiver"));
        assert!(!unbound.contains("tab-1"));
        assert_eq!(render(&json!({ "plugins": [] })), None);
    }
}
