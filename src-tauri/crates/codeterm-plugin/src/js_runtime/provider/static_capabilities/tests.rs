use super::*;
use crate::js_runtime::call_gate;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn immutable_capabilities_do_not_wait_for_a_held_provider_vm() {
    let entered = "static-capabilities.entered";
    let release = "static-capabilities.release";
    call_gate::reset(entered);
    call_gate::reset(release);
    let manifest: PluginManifest = serde_json::from_value(
        json!({"id":"static-capability-fixture","version":"0.0.0","displayName":"Fixture"}),
    )
    .unwrap();
    let vm = Arc::new(PluginVm::from_source(&manifest.id,
        "module.exports={authStatusProbe(){return {args:['opaque'],timeoutMs:1};},tracksBackgroundWork(){return true;},usesStructuredChat(){return true;},block(e,r){host.testGateOpen(e);host.testGateBlock(r);}};".into()).unwrap());
    let provider = Arc::new(JsProvider::new(manifest, vm.clone()));
    let holder = std::thread::spawn(move || {
        call_json::<Value>(&vm, "block", &[json!(entered), json!(release)])
    });
    assert!(call_gate::wait_opened(entered, Duration::from_secs(1)));
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        tx.send((
            provider.auth_status_probe().unwrap().args,
            provider.tracks_background_work(),
            provider.uses_structured_chat(),
        ))
        .unwrap();
    });
    let result = rx.recv_timeout(Duration::from_secs(1));
    call_gate::open(release);
    reader.join().unwrap();
    let _held_result = holder.join().unwrap();
    assert_eq!(result.unwrap(), (vec!["opaque".to_string()], true, true));
}
