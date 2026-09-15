use std::sync::Arc;

use serde_json::Value;

use super::tests::{scene, RECEIVER};
use super::{host_json, ReceiverTargetError, ReceiverTargetView};
use crate::service::ServiceContext;

const PROBE_PLUGIN: &str = r#"module.exports.default = {
    read() { return host.receiverTarget.get(); },
    reset() { return host.receiverTarget.clear(); },
}"#;

fn js_reply(service: &ServiceContext, plugin_id: &str, method: &str) -> Value {
    let host: Arc<dyn codeterm_plugin::PluginHostCtx> = Arc::new(service.clone());
    let vm =
        codeterm_plugin::js_runtime::PluginVm::from_source(plugin_id, PROBE_PLUGIN.to_string())
            .expect("probe vm");
    assert!(vm.set_service(Arc::downgrade(&host)));
    codeterm_plugin::js_runtime::call_json(&vm, method, &[]).expect("probe call")
}

fn native_reply(result: Result<ReceiverTargetView, ReceiverTargetError>) -> Value {
    serde_json::from_str(&host_json(result)).unwrap()
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn plugin_parity_receiver_target_host_and_native_paths_agree() {
    let scene = scene();
    let service = &scene.fixture.service;

    assert_eq!(
        js_reply(service, RECEIVER, "read"),
        native_reply(service.receiver_target(RECEIVER))
    );

    service.set_receiver_target(RECEIVER, &scene.tab).unwrap();
    let bound = js_reply(service, RECEIVER, "read");
    assert_eq!(bound, native_reply(service.receiver_target(RECEIVER)));
    assert_eq!(bound["tabId"], scene.tab.as_str());

    assert_eq!(
        js_reply(service, "other-plugin", "read"),
        native_reply(service.receiver_target("other-plugin"))
    );
    assert_eq!(
        js_reply(service, "other-plugin", "reset")["error"],
        "receiver_not_found"
    );
    assert_eq!(
        service.receiver_targets.get(RECEIVER),
        Some(scene.tab.clone()),
        "a plugin cannot clear another receiver's binding"
    );

    let reset = js_reply(service, RECEIVER, "reset");
    assert_eq!(reset, native_reply(service.clear_receiver_target(RECEIVER)));
    assert_eq!(service.receiver_targets.get(RECEIVER), None);
}
