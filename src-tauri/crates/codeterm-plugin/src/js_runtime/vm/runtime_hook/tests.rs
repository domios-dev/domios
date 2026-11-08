use super::*;
use crate::js_runtime::call_gate;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) fn profile_invocation(method: &str, source: &str) -> Option<String> {
    if method != "runtimeReduce" || std::env::var_os("CODETERM_CHILD_RETENTION_TIMING").is_none() {
        return None;
    }
    Some(
        source
            .replace(
                "  const args = JSON.parse",
                "  const profileStarted = Date.now();\n  const args = JSON.parse",
            )
            .replace(
                "  const value = fn.apply",
                "  const profileParsed = Date.now();\n  const value = fn.apply",
            )
            .replace(
                "  if (globalThis.__ct_runtime_denied_binding__",
                "  const profileReduced = Date.now();\n  if (globalThis.__ct_runtime_denied_binding__",
            )
            .replace(
                "  return result;",
                "  globalThis.__ct_runtime_profile__ = JSON.stringify({parse_ms:profileParsed-profileStarted,reducer_ms:profileReduced-profileParsed,stringify_ms:Date.now()-profileReduced});\n  return result;",
            ),
    )
}

pub(super) fn report_profile(ctx: &rquickjs::Ctx<'_>) {
    if let Ok(Some(profile)) = ctx
        .globals()
        .get::<_, Option<String>>("__ct_runtime_profile__")
    {
        eprintln!("CHILD_RETENTION_EXECUTE {profile}");
    }
    let _ = ctx.globals().remove("__ct_runtime_profile__");
}

#[test]
fn runtime_argument_serialization_preserves_exact_byte_boundaries() {
    for value in [
        json!(null),
        json!(["\0\n\"\\", "é雪😀"]),
        json!({"nested":[1,true,null]}),
    ] {
        let expected = serde_json::to_string(&value).unwrap();
        let exact = crate::agent_runtime::serialize_runtime_value(&value, expected.len()).unwrap();
        assert_eq!(exact, expected);
        let limit = expected.len() - 1;
        let expected_error =
            crate::agent_runtime::validate_runtime_size(&value, limit).unwrap_err();
        let actual_error =
            crate::agent_runtime::serialize_runtime_value(&value, limit).unwrap_err();
        assert_eq!(actual_error.code, expected_error.code);
        assert_eq!(actual_error.message, expected_error.message);
    }
}

#[test]
fn oversized_runtime_arguments_refuse_before_context_initialization() {
    let vm = PluginVm::from_source(
        "argument-budget",
        "module.exports={runtimePlan(){throw new Error('must not execute')}}".into(),
    )
    .unwrap();
    let error = call_runtime_hook::<Value>(
        &vm,
        "runtimePlan",
        &[json!("x".repeat(MAX_RUNTIME_VALUE_BYTES))],
        Instant::now() + Duration::from_secs(60),
    )
    .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::SizeLimit);
    assert!(vm.runtime_inner.lock().is_none());
}

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

#[test]
fn host_mcp_credential_cannot_escape_through_hook_errors_or_state() {
    let context = json!({"hostMcp":{"env":{"CODETERM_API_TOKEN":"<credential-placeholder>"}}});
    for body in [
        "throw new Error(ctx.hostMcp.env.CODETERM_API_TOKEN)",
        "return {state:{copied:ctx.hostMcp.env.CODETERM_API_TOKEN}}",
        "return {action:{kind:'refused',reason:ctx.hostMcp.env.CODETERM_API_TOKEN}}",
        "return {events:[{message:ctx.hostMcp.env.CODETERM_API_TOKEN}]}",
        "return {kind:'continue',plan:{state:{copied:ctx.hostMcp.env.CODETERM_API_TOKEN}}}",
    ] {
        let vm = PluginVm::from_source(
            "credential-boundary",
            format!("module.exports={{runtimePlan(ctx){{{body}}}}}"),
        )
        .unwrap();
        let error = call_runtime_hook::<Value>(
            &vm,
            "runtimePlan",
            &[context.clone()],
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(!error.message.contains("<credential-placeholder>"));
    }
}

#[test]
fn host_mcp_credential_is_allowed_only_in_exact_outbound_descriptor() {
    let descriptor = crate::agent_runtime::RuntimeHostMcp {
        name: "domios".into(),
        command: "/trusted/domios".into(),
        args: vec!["mcp".into(), "serve".into()],
        env: std::collections::BTreeMap::from([(
            "CODETERM_API_TOKEN".into(),
            "<credential-placeholder>".into(),
        )]),
    };
    let context = json!({"hostMcp":descriptor});
    let vm = PluginVm::from_source("descriptor-boundary", "module.exports={runtimePlan(ctx,op){return {action:{kind:'request',call:{method:'session/new',params:{mcpServers:op.servers}}},state:{method:'session/new'}}}}".into()).unwrap();
    let servers = json!([descriptor.descriptor()]);
    let value = call_runtime_hook::<Value>(
        &vm,
        "runtimePlan",
        &[context.clone(), json!({"servers":servers})],
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(value["action"]["call"]["params"]["mcpServers"], servers);
    let mut changed = servers;
    changed[0]["command"] = json!("/foreign");
    let error = call_runtime_hook::<Value>(
        &vm,
        "runtimePlan",
        &[context, json!({"servers":changed})],
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap_err();
    assert_eq!(error.code, RuntimeFailureCode::InvalidSchema);
}
