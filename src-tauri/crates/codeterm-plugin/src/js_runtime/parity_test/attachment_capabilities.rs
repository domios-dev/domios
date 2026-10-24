use super::*;
use crate::provider::AttachmentCapabilities;

#[test]
fn attachment_capabilities_native_js_parity() {
    let native = super::reply_relay::RelayNative;
    let manifest = serde_json::json!({
        "id": "attachment-fixture", "version": "0.1.0", "displayName": "Fixture",
        "capabilities": { "agentProvider": true,
            "attachments": { "filePaths": true, "inlineText": true, "imagePaths": true } },
        "spawn": { "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
            "taskDelivery": "pty_input", "verifySubmit": { "kind": "none" } }
    })
    .to_string();
    let js = load_inline_js_with_manifest(
        "attachment-fixture",
        &manifest,
        "module.exports.default = {};",
    );
    assert_eq!(
        js.attachment_capabilities(),
        native.attachment_capabilities()
    );
    assert_eq!(js.describe().attachments, native.describe().attachments);
    let undeclared = load_inline_js("undeclared-attachments", "module.exports.default = {};");
    assert_eq!(
        undeclared.attachment_capabilities(),
        DiscoverModelsNativeStub.attachment_capabilities()
    );
    assert_eq!(
        undeclared.describe().attachments,
        AttachmentCapabilities::default()
    );
    assert!(!undeclared.describe().attachments.file_paths);
    assert!(!undeclared.describe().attachments.image_paths);
}
