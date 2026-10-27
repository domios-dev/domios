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
    assert!(working
        .message
        .starts_with("runtimeResponse: runtime hook execution deadline:"));
    assert!(
        working.message.contains("interrupted"),
        "{}",
        working.message
    );
    assert!(vm.runtime_inner.lock().is_none());
    call_gate::reset(gate);

    let mut failing_args = args;
    failing_args[1] = json!({"kind":"readHistory","fail":true});
    let projection =
        call_runtime_hook_with_clock::<Value>(&vm, "runtimeResponse", &failing_args, expired, now)
            .unwrap_err();
    assert_eq!(projection.code, RuntimeFailureCode::HookFailed);
    assert!(projection.message.contains("runtimeResponse: "));
    assert!(projection.message.contains("projection failed"));
    assert!(!call_gate::is_open(gate));
    call_gate::reset(gate);
}

#[test]
fn interrupted_cached_hook_recovers_on_a_fresh_context() {
    let gate = "cached-hook-interrupt";
    call_gate::reset(gate);
    let vm = PluginVm::from_source(
        "interrupt-recovery",
        r#"
      let calls=0;
      module.exports={runtimePlan(ctx,op){
        calls++;
        if(op.phase==="interrupt"){
          __runtime_test_gate__.testGateOpen('cached-hook-interrupt');
          for(;;){}
        }
        return calls;
      }};
    "#
        .into(),
    )
    .unwrap();
    assert_eq!(run_phase(&vm, "launch").unwrap(), json!(1));
    let deadline = Instant::now();
    let before = deadline - Duration::from_secs(1);
    let clock: HookClock = Arc::new(move || {
        if call_gate::is_open(gate) {
            deadline
        } else {
            before
        }
    });
    let error = call_runtime_hook_with_clock::<Value>(
        &vm,
        "runtimePlan",
        &[Value::Null, json!({"phase":"interrupt"})],
        deadline,
        clock,
    )
    .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::Deadline);
    assert!(error.message.contains("runtimePlan"));
    assert!(error.message.contains("interrupted"), "{}", error.message);
    assert!(vm.runtime_inner.lock().is_none());
    call_gate::reset(gate);
    assert_eq!(run_phase(&vm, "launch").unwrap(), json!(1));
}

#[test]
fn thrown_errors_retire_cached_globals_and_preserve_exception_text() {
    for (expression, expected) in [
        ("new Error('projection failed')", "projection failed"),
        ("new Error('')", "runtimePlan: Error"),
        (
            "{code:'provider_refused',message:'owned refusal'}",
            "owned refusal",
        ),
        ("null", "Null"),
    ] {
        let source = format!("let calls=0;module.exports={{runtimePlan(ctx,op){{calls++;if(op.phase==='fail')throw {expression};return calls;}}}}");
        let vm = PluginVm::from_source("throw-recovery", source).unwrap();
        assert_eq!(run_phase(&vm, "launch").unwrap(), json!(1));
        assert_eq!(run_phase(&vm, "launch").unwrap(), json!(2));
        let error = run_phase(&vm, "fail").unwrap_err();
        assert_eq!(error.code, RuntimeFailureCode::HookFailed);
        assert!(error.message.starts_with("runtimePlan: "));
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(vm.runtime_inner.lock().is_none());
        assert_eq!(run_phase(&vm, "launch").unwrap(), json!(1));
    }
}

#[test]
fn out_of_memory_hook_recovers_without_retaining_its_heap() {
    let vm = PluginVm::from_source(
        "oom-recovery",
        r#"
      let calls=0;const retained=[];
      module.exports={runtimePlan(ctx,op){
        calls++;
        if(op.phase==="oom")for(let i=0;i<200000;i++)retained.push("x".repeat(1000)+i);
        return calls;
      }};
    "#
        .into(),
    )
    .unwrap();
    assert_eq!(run_phase(&vm, "launch").unwrap(), json!(1));
    let error = run_phase(&vm, "oom").unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::HookFailed);
    assert!(error.message.starts_with("runtimePlan: "));
    assert!(
        error.message.contains("out of memory")
            || error.message == "runtimePlan: InternalError"
            || error.message.contains("Value(Null"),
        "{}",
        error.message
    );
    assert!(vm.runtime_inner.lock().is_none());
    assert_eq!(run_phase(&vm, "launch").unwrap(), json!(1));
}

#[test]
fn null_runtime_plan_is_rejected_and_retires_the_context() {
    let vm = PluginVm::from_source(
        "null-plan",
        "module.exports={runtimePlan(){return null}}".into(),
    )
    .unwrap();
    let error = call_runtime_hook::<crate::agent_runtime::RuntimePlan>(
        &vm,
        "runtimePlan",
        &[],
        Instant::now() + Duration::from_secs(60),
    )
    .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::InvalidSchema);
    assert!(error.message.starts_with("runtimePlan: "));
    assert!(vm.runtime_inner.lock().is_none());
}

fn run_phase(vm: &PluginVm, phase: &str) -> Result<Value, RuntimeFailure> {
    call_runtime_hook(
        vm,
        "runtimePlan",
        &[Value::Null, json!({ "phase": phase })],
        Instant::now() + Duration::from_secs(60),
    )
}

#[test]
fn runtime_context_survives_cyclic_garbage_after_a_peak_heap_call() {
    let vm = PluginVm::from_source(
        "gc-wedge",
        r#"module.exports={runtimePlan(ctx,op){
          if(op.phase==="peak"){const keep=[];for(let i=0;i<44000;i++)keep.push("x".repeat(1000)+i);keep.push({});return keep.length;}
          if(op.phase==="fill"){
            try{for(let i=0;i<7000;i++){const a={s:"y".repeat(10000)+i};a.self=a;}}catch(e){}
            try{for(let i=0;i<1000000;i++){const o={};o.self=o;}}catch(e){}
            return 0;
          }
          return "ok";
        }}"#
        .into(),
    )
    .unwrap();
    assert_eq!(run_phase(&vm, "peak").unwrap(), json!(44001));
    assert_eq!(run_phase(&vm, "fill").unwrap(), json!(0));
    assert_eq!(run_phase(&vm, "launch").unwrap(), json!("ok"));
}

#[test]
fn runtime_setup_failure_reports_the_js_exception_and_resets_the_context() {
    let vm = PluginVm::from_source(
        "setup-failure",
        r#"Object.defineProperty(globalThis,"__ct_runtime_method__",{value:"locked",writable:false});
        module.exports={runtimePlan(){return "ok";}}"#
            .into(),
    )
    .unwrap();
    let error = run_phase(&vm, "launch").unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::HookFailed);
    assert!(error.message.starts_with("runtimePlan setup: "));
    assert!(error.message.contains("read-only"), "{}", error.message);
    assert!(vm.runtime_inner.lock().is_none());
}
