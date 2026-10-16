use super::*;
use crate::js_runtime::call_gate;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn runtime_history_deadline_distinguishes_vm_contention_from_projection() {
    let expired = Instant::now();
    let vm = PluginVm::from_source("history-budget", "module.exports={runtimeResponse(ctx,op,state,reply){if(op.fail)throw new Error('projection failed');__runtime_test_gate__.testGateOpen('history-projection-deadline');for(const row of reply.value.data){for(;;){JSON.stringify(row)}}}}".into()).unwrap();
    let args = [
        Value::Null,
        json!({"kind":"readHistory"}),
        Value::Null,
        json!({"value":{"data":[{"text":"x".repeat(16 * 1024)}]}}),
    ];
    let guard = vm.runtime_inner.lock();
    let waiting = call_runtime_hook_with_clock::<Value>(
        &vm,
        "runtimeResponse",
        &args,
        expired,
        Arc::new(move || expired),
    )
    .unwrap_err();
    drop(guard);
    assert_eq!(waiting.code, RuntimeFailureCode::Deadline);
    assert_eq!(waiting.message, "runtime VM lock deadline");

    let gate = "history-projection-deadline";
    call_gate::reset(gate);
    let before = expired - Duration::from_millis(20);
    let now: HookClock = Arc::new(move || {
        if call_gate::is_open(gate) {
            expired
        } else {
            before
        }
    });
    let working =
        call_runtime_hook_with_clock::<Value>(&vm, "runtimeResponse", &args, expired, now.clone())
            .unwrap_err();
    assert!(call_gate::is_open(gate));
    assert_eq!(working.code, RuntimeFailureCode::Deadline);
    assert_eq!(working.message, "runtime hook execution deadline");
    call_gate::reset(gate);

    let mut failing_args = args;
    failing_args[1] = json!({"kind":"readHistory","fail":true});
    let projection =
        call_runtime_hook_with_clock::<Value>(&vm, "runtimeResponse", &failing_args, expired, now)
            .unwrap_err();
    assert_eq!(projection.code, RuntimeFailureCode::HookFailed);
    assert!(!call_gate::is_open(gate));
    call_gate::reset(gate);
}
