// WHAT: native and JS confirmed-close acknowledgements on shared session fixtures.
// WHY: optional best-effort teardown must not authorize a replacement lane writer.
// ORACLE: exact-session exit receipts pass; missing, failed and foreign receipts refuse.
use crate::chatbackend_provider::{ChatBackendError, ChatBackendProvider};
use crate::js_runtime::{JsChatBackend, PluginVm};
use crate::provider::ModelInfo;
use codeterm_types::agent::chat_types::StructuredChat;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct NativeClose {
    session: &'static str,
    exited: bool,
}

impl ChatBackendProvider for NativeClose {
    fn id(&self) -> &str {
        "close-fixture"
    }
    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!()
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!()
    }
    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        unreachable!()
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!()
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        unreachable!()
    }
    fn close_session_confirmed(
        &self,
        session: &str,
        deadline: Instant,
    ) -> Result<(), ChatBackendError> {
        if self.session == session && self.exited && Instant::now() < deadline {
            Ok(())
        } else {
            Err(ChatBackendError::Call("close not confirmed".into()))
        }
    }
}

#[test]
fn confirmed_chat_close_native_js_parity() {
    for (session, exited, accepted) in [
        ("owned", true, true),
        ("foreign", true, false),
        ("owned", false, false),
    ] {
        let native = NativeClose { session, exited };
        let source = format!("module.exports.default = {{ closeSession() {{ return {{sessionId: {}, processExited: {exited}}}; }} }};", serde_json::to_string(session).unwrap());
        let vm = Arc::new(PluginVm::from_source("close-parity", source).unwrap());
        let js = JsChatBackend::new(vm, "close-fixture");
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            native.close_session_confirmed("owned", deadline).is_ok(),
            accepted
        );
        assert_eq!(
            js.close_session_confirmed("owned", deadline).is_ok(),
            accepted
        );
    }
}

#[test]
fn confirmed_chat_close_refuses_optional_and_failed_plugin_exports() {
    for source in [
        "module.exports.default = {};",
        "module.exports.default = { closeSession() {} };",
        "module.exports.default = { closeSession() { throw new Error('close failed'); } };",
        "module.exports.default = { closeSession() { return {error:'not closed'}; } };",
    ] {
        let vm = Arc::new(PluginVm::from_source("close-negative", source.into()).unwrap());
        let js = JsChatBackend::new(vm, "close-fixture");
        assert!(js
            .close_session_confirmed("owned", Instant::now() + Duration::from_secs(5))
            .is_err());
    }
}

#[test]
fn confirmed_chat_close_refuses_a_wedged_adapter_at_its_deadline() {
    let vm = Arc::new(
        PluginVm::from_source(
            "close-deadline",
            "module.exports.default = { closeSession() { for (;;) {} } };".into(),
        )
        .unwrap(),
    );
    let js = JsChatBackend::new(vm, "close-fixture");
    assert!(js.close_session_confirmed("owned", Instant::now()).is_err());
}

#[test]
fn confirmed_chat_close_awaits_the_adapter_exit_job() {
    use crate::js_runtime::{call_gate, vm::call_json};
    let entered = "confirmed-close.entered";
    let release = "confirmed-close.release";
    call_gate::reset(entered);
    call_gate::reset(release);
    struct Release(&'static str);
    impl Drop for Release {
        fn drop(&mut self) {
            call_gate::open(self.0);
        }
    }
    let _release = Release(release);
    let plugin = "close-await-exit";
    let source = format!("let closed=false; module.exports.default = {{ closed() {{ return closed; }}, closeSession(id) {{ const job=host.testGateStart({release:?}); host.testGateOpen({entered:?}); return host.awaitJob(job, function(result) {{ if (result.code !== 0) throw new Error('exit failed'); closed=true; return {{sessionId:id, processExited:true}}; }}); }} }};");
    let vm = Arc::new(PluginVm::from_source(plugin, source).unwrap());
    let js = JsChatBackend::new(vm.clone(), plugin);
    let close = std::thread::spawn(move || {
        js.close_session_confirmed("owned", Instant::now() + Duration::from_secs(5))
    });
    assert!(call_gate::wait_opened(entered, Duration::from_secs(5)));
    assert!(!call_json::<bool>(&vm, "closed", &[]).unwrap());
    call_gate::open(release);
    close.join().unwrap().unwrap();
    assert!(call_json::<bool>(&vm, "closed", &[]).unwrap());
}

#[test]
fn confirmed_chat_close_awaits_a_native_backend_process() {
    #[cfg(windows)]
    let opts = serde_json::json!({"bin":"powershell", "args":["-NoProfile", "-NonInteractive", "-Command", "exit 0"], "timeoutMs":4000});
    #[cfg(not(windows))]
    let opts = serde_json::json!({"bin":"sh", "args":["-c", "exit 0"], "timeoutMs":4000});
    let source = format!("module.exports.default = {{ closeSession(id) {{ return host.exec.async({opts}, function(result) {{ if (result.code !== 0 || result.error) throw new Error('exit failed'); return {{sessionId:id, processExited:true}}; }}); }} }};");
    let vm = Arc::new(PluginVm::from_source("close-native-exit", source).unwrap());
    let js = JsChatBackend::new(vm, "close-native-exit");
    js.close_session_confirmed("owned", Instant::now() + Duration::from_secs(5))
        .unwrap();
}
