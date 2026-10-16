use super::*;
use serde_json::{json, Value};

#[test]
fn runtime_history_deadline_distinguishes_vm_contention_from_projection() {
    let vm = PluginVm::from_source("history-budget", "module.exports={runtimeResponse(ctx,op,state,reply){for(const row of reply.value.data){for(;;){JSON.stringify(row)}}}}".into()).unwrap();
    let args = [
        Value::Null,
        json!({"kind":"readHistory"}),
        Value::Null,
        json!({"value":{"data":[{"text":"x".repeat(16 * 1024)}]}}),
    ];
    let invoke = || {
        crate::js_runtime::vm::call_runtime_hook::<Value>(
            &vm,
            "runtimeResponse",
            &args,
            std::time::Instant::now() + std::time::Duration::from_millis(20),
        )
    };
    let guard = vm.runtime_inner.lock();
    let waiting = invoke().unwrap_err();
    drop(guard);
    assert_eq!(waiting.code, RuntimeFailureCode::Deadline);
    assert_eq!(waiting.message, "runtime VM lock deadline");
    let working = invoke().unwrap_err();
    assert_eq!(working.code, RuntimeFailureCode::Deadline);
    assert_eq!(working.message, "runtime hook execution deadline");
}
