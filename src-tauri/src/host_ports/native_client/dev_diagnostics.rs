use codeterm_types::{
    native_host::NativeWebviewDiagnosticsSnapshot, server_bootstrap::ServerReady,
};
use serde_json::{json, Value};

pub(crate) fn attachment_receipt(ready: &ServerReady, client_pid: u32) -> String {
    let receipt = json!({
        "protocol": ready.protocol,
        "client_pid": client_pid,
        "server_pid": ready.process.pid,
        "epoch": ready.epoch,
        "api_address": ready.api_address.to_string(),
    });
    format!("DOMIOS_DEV_ATTACHMENT {receipt}")
}

pub(super) fn root_receipts(
    ready: &ServerReady,
    snapshot: &NativeWebviewDiagnosticsSnapshot,
    navigation_generation: u64,
    preflight: Value,
    origin: &str,
) -> String {
    let attachment = attachment_receipt(ready, snapshot.client_process.pid);
    let locator = json!({
        "snapshot": snapshot,
        "navigation_generation": navigation_generation,
        "preflight": preflight,
        "origin": origin,
    });
    format!("{attachment}\nDOMIOS_DEV_NATIVE_LOCATOR {locator}")
}

#[cfg(test)]
mod tests;
