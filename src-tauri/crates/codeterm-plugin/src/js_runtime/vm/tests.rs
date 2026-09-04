//! Tests for the plugin VM call bridge.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::js_runtime::call_gate;

fn fixture(src: &str) -> PluginVm {
    PluginVm::from_source("test", src.to_string()).expect("load")
}

#[test]
fn round_trip_string_arg_and_return() {
    let vm = fixture(
        r#"
        module.exports.default = {
            upper(s) { return s.toUpperCase(); }
        };
    "#,
    );
    let out: String = call_json(&vm, "upper", &[json!("hi")]).unwrap();
    assert_eq!(out, "HI");
}

#[test]
fn round_trip_object_arg_and_return() {
    let vm = fixture(
        r#"
        module.exports.default = {
            meta() { return { id: "x", displayName: "X" }; }
        };
    "#,
    );
    let out: serde_json::Value = call_json(&vm, "meta", &[]).unwrap();
    assert_eq!(out["id"], "x");
    assert_eq!(out["displayName"], "X");
}

#[test]
fn round_trip_array_return() {
    let vm = fixture(
        r#"
        module.exports.default = {
            things() { return [1, 2, 3]; }
        };
    "#,
    );
    let out: Vec<i32> = call_json(&vm, "things", &[]).unwrap();
    assert_eq!(out, vec![1, 2, 3]);
}

#[test]
fn missing_function_errors() {
    let vm = fixture("module.exports.default = {};");
    let err = call_json::<()>(&vm, "nope", &[]).unwrap_err();
    match err {
        VmError::Script(_) | VmError::MissingFunction(_) => {}
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn plugin_throw_surfaces_as_script_error() {
    let vm = fixture(
        r#"
        module.exports.default = {
            kaboom() { throw new Error('nope'); }
        };
    "#,
    );
    let err = call_json::<()>(&vm, "kaboom", &[]).unwrap_err();
    assert!(matches!(err, VmError::Script(_)));
}

#[test]
fn returning_undefined_becomes_null_in_rust() {
    let vm = fixture(
        r#"
        module.exports.default = {
            nothing() { /* implicit undefined */ }
        };
    "#,
    );
    let out: Option<i32> = call_json(&vm, "nothing", &[]).unwrap();
    assert_eq!(out, None);
}

#[test]
fn promise_return_rejected() {
    let vm = fixture(
        r#"
        module.exports.default = {
            async slow() { return 1; }
        };
    "#,
    );
    let err = call_json::<i32>(&vm, "slow", &[]).unwrap_err();
    assert!(matches!(err, VmError::Script(_)));
}

#[test]
fn boolean_return() {
    let vm = fixture(
        r#"
        module.exports.default = {
            yes() { return true; },
            no()  { return false; }
        };
    "#,
    );
    let yes: bool = call_json(&vm, "yes", &[]).unwrap();
    let no: bool = call_json(&vm, "no", &[]).unwrap();
    assert!(yes);
    assert!(!no);
}

#[test]
fn hot_path_no_per_call_recompile() {
    // try_call("hasEventMarkers") -> call_json, fired per output chunk (#186).
    let vm = fixture(
        r#"
        module.exports.default = {
            hasEventMarkers(chunk) {
                return chunk.includes("❯")
                    || chunk.includes("Proceed?")
                    || (chunk.includes("1.") && chunk.includes("2."));
            }
        };
    "#,
    );
    let chunk = "\u{1b}[2mthinking...\u{1b}[0m\n\u{276f} 1. Yes  2. No\nProceed?";
    // Warm one call so lazy state settles before the snapshot.
    let _: bool = call_json(&vm, "hasEventMarkers", &[json!(chunk)]).unwrap();
    let before = source_compiles();
    let n = 20_000u64;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let hit: bool = call_json(&vm, "hasEventMarkers", &[json!(chunk)]).unwrap();
        assert!(hit);
    }
    let elapsed = t0.elapsed();
    let compiles = source_compiles() - before;
    eprintln!(
        "[#186 hot-path load] {n} calls in {elapsed:?} -> {:.3} us/call, {:.0} calls/sec; \
         source recompiles during loop = {compiles}",
        elapsed.as_micros() as f64 / n as f64,
        n as f64 / elapsed.as_secs_f64(),
    );
    assert_eq!(
        compiles, 0,
        "per-call JS source recompile detected (#186): call_json must invoke a cached \
         wrapper Function, not ctx.eval(source) on every call"
    );
}

#[test]
fn esm_export_default_style() {
    // esbuild output for `export default { ... }` produces `module.exports.default = ...` after the bootstrap interpret.
    let vm = fixture(
        r#"
        Object.defineProperty(module.exports, "__esModule", { value: true });
        module.exports.default = {
            hello() { return "world"; }
        };
    "#,
    );
    let out: String = call_json(&vm, "hello", &[]).unwrap();
    assert_eq!(out, "world");
}

#[test]
fn preamble_exposes_host_fs_and_host_manifest() {
    // would be undefined and these calls would throw TypeError.
    let vm = PluginVm::from_source_with(
        "test",
        r#"
        module.exports.default = {
            listNames(p) { return host.fs.readDir(p).map(e => e.name).sort(); },
            getManifestId() {
                var m = host.manifest();
                return m ? m.id : null;
            },
            wrap(s) { return host.shell.quote(s); }
        };
        "#
        .to_string(),
        Some(r#"{"id":"X","version":"0","displayName":"X","spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}}"#.to_string()),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), "1").unwrap();
    std::fs::write(dir.path().join("b"), "2").unwrap();
    let names: Vec<String> =
        call_json(&vm, "listNames", &[json!(dir.path().to_string_lossy())]).unwrap();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    let id: Option<String> = call_json(&vm, "getManifestId", &[]).unwrap();
    assert_eq!(id.as_deref(), Some("X"));
    let q: String = call_json(&vm, "wrap", &[json!("can't")]).unwrap();
    assert_eq!(q, "'can'\\''t'");
}

#[test]
fn preamble_exposes_host_toolcall_parse() {
    let vm = fixture(
        r#"
        module.exports.default = {
            parse(raw, schema) { return host.toolcall.parse(raw, schema); }
        };
        "#,
    );
    let schema = r#"{
        "tools":[{"name":"exec","args":["cmd"]}],
        "aliases":{"command":"cmd"}
    }"#;
    let raw = r#"```codeterm-tool
{"tool":"exec","args":{"command":"ls"}}
```"#;

    let via_host: String = call_json(&vm, "parse", &[json!(raw), json!(schema)]).unwrap();
    let native = super::super::toolcall_parse::parse_to_json(raw, schema);
    assert_eq!(via_host, native);
    let parsed: serde_json::Value = serde_json::from_str(&via_host).unwrap();
    assert_eq!(parsed["tool"], "exec");
    assert_eq!(parsed["args"]["cmd"], "ls");
}

#[test]
fn preamble_compute_is_fully_synchronous() {
    // All compute binds return their result directly, never a Promise (undrainable in a sync dispatch). Assert the shape.
    let vm = fixture(
        r#"
        module.exports.default = {
            shape() {
                var mem = host.mem.search({ query: "q" });
                var start = host.worker.start({ task: "t" });
                var send = host.agent.send("s", "t");
                return {
                    memIsFn:       typeof host.mem.search === "function",
                    wsEnsureIsFn:  typeof host.workspace.ensure === "function",
                    wsTabsIsFn:   typeof host.workspace.tabs === "function",
                    // every result is a plain object, NOT a thenable.
                    memSync:       mem !== null && typeof mem === "object" && mem.then === undefined,
                    memError:      typeof mem.error === "string",
                    workerSync:    start !== null && typeof start === "object" && start.then === undefined,
                    agentSync:     send !== null && typeof send === "object" && send.then === undefined,
                };
            }
        };
        "#,
    );
    let v: serde_json::Value = call_json(&vm, "shape", &[]).unwrap();
    assert_eq!(v["memIsFn"], true);
    assert_eq!(v["wsEnsureIsFn"], true);
    assert_eq!(v["wsTabsIsFn"], true);
    assert_eq!(v["memSync"], true, "host.mem.search must be synchronous");
    assert_eq!(
        v["memError"], true,
        "svc-absent must surface an error envelope"
    );
    assert_eq!(
        v["workerSync"], true,
        "host.worker.start must be synchronous (no Promise wrapper)"
    );
    assert_eq!(
        v["agentSync"], true,
        "host.agent.send must be synchronous (no Promise wrapper)"
    );
}

#[test]
fn host_compute_perms_gate_workspace_and_agent() {
    // host.workspace* needs manage_workspaces, host.agent* needs spawn_agents: without the grant the bind denies; with it, the gate is crossed.
    use crate::js_runtime::host_api::PluginSandbox;
    use crate::js_runtime::manifest::PermissionsConfig;
    use std::sync::Arc;
    const SRC: &str = r#"
        module.exports.default = {
            ws() { return host.workspaceEnsure("{}"); },
            ag() { return host.agentSpawn("w", "{}"); }
        };
    "#;

    let denied = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms("p", &PermissionsConfig::default()),
    );
    let vm = PluginVm::from_source_sandboxed("p", SRC.to_string(), None, denied).unwrap();
    let ws: String = call_json(&vm, "ws", &[]).unwrap();
    assert!(ws.contains("manage_workspaces permission required"), "{ws}");
    let ag: String = call_json(&vm, "ag", &[]).unwrap();
    assert!(ag.contains("spawn_agents permission required"), "{ag}");

    let perms = PermissionsConfig {
        manage_workspaces: true,
        spawn_agents: true,
        ..Default::default()
    };
    let granted =
        Arc::new(PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms("p", &perms));
    let vm2 = PluginVm::from_source_sandboxed("p", SRC.to_string(), None, granted).unwrap();
    let ws2: String = call_json(&vm2, "ws", &[]).unwrap();
    assert!(
        ws2.contains("host service unavailable"),
        "granted ws: {ws2}"
    );
    let ag2: String = call_json(&vm2, "ag", &[]).unwrap();
    assert!(
        ag2.contains("host service unavailable"),
        "granted ag: {ag2}"
    );
}

#[test]
fn fetch_stream_preamble_is_sync_and_uses_fetch_allowlist() {
    use crate::js_runtime::host_api::PluginSandbox;
    use crate::js_runtime::manifest::PermissionsConfig;
    use std::sync::Arc;

    const SRC: &str = r#"
        module.exports.default = {
            shape() {
                var result = host.fetch.stream({ url: "http://blocked.example.test/stream" });
                return {
                    streamIsFn: typeof host.fetch.stream === "function",
                    pollIsFn: typeof host.fetch.streamPoll === "function",
                    closeIsFn: typeof host.fetch.streamClose === "function",
                    sync: !(result && typeof result.then === "function"),
                    error: result && result.error,
                };
            }
        };
    "#;

    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms("p", &PermissionsConfig::default()),
    );
    let vm = PluginVm::from_source_sandboxed("p", SRC.to_string(), None, sandbox).unwrap();
    let shape: serde_json::Value = call_json(&vm, "shape", &[]).unwrap();
    assert_eq!(shape["streamIsFn"], true);
    assert_eq!(shape["pollIsFn"], true);
    assert_eq!(shape["closeIsFn"], true);
    assert_eq!(shape["sync"], true);
    assert_eq!(
        shape["error"], "fetch denied: blocked.example.test",
        "stream must share host.fetch's network allowlist denial"
    );
}

#[test]
fn reload_swaps_implementation() {
    let vm = fixture(
        r#"
        module.exports.default = { v() { return 1; } };
    "#,
    );
    let v1: i32 = call_json(&vm, "v", &[]).unwrap();
    assert_eq!(v1, 1);

    vm.reload(
        r#"
        module.exports.default = { v() { return 2; } };
    "#
        .to_string(),
    )
    .unwrap();
    let v2: i32 = call_json(&vm, "v", &[]).unwrap();
    assert_eq!(v2, 2);
}

// ── VM serialization: a slow call must not hold the context for its whole wait ──

/// The fixture branches on `host.awaitJob` so the same test also runs against a
/// pre-fix tree, where the only shape available is blocking inside the frame.
const SERIALIZATION_FIXTURE: &str = r#"
var trace = [];
var depth = 0;
function enter(tag) {
  depth += 1;
  if (depth > 1) { trace.push("INTERLEAVED:" + tag); throw new Error("two frames live in one context"); }
  trace.push(tag);
}
function leave() { depth -= 1; }

module.exports.default = {
  fast: function (tag) { enter("fast:" + tag); leave(); return 7; },
  gated: function (entered, release, tag) {
    enter(tag);
    if (typeof host.awaitJob !== "function") {
      host.testGateOpen(entered);
      host.testGateBlock(release);
      leave();
      return { tag: tag, code: 0 };
    }
    var job = host.testGateStart(release);
    host.testGateOpen(entered);
    leave();
    return host.awaitJob(job, function (r) {
      enter(tag + ":resume");
      leave();
      return { tag: tag, code: r.code };
    });
  },
  bounded: function (release) {
    var job = host.testGateStart(release);
    return host.awaitJob(job, function () { return { state: "completed" }; });
  },
  boom: function (entered, release) {
    if (typeof host.awaitJob !== "function") {
      host.testGateOpen(entered);
      host.testGateBlock(release);
      throw new Error("continuation blew up");
    }
    var job = host.testGateStart(release);
    host.testGateOpen(entered);
    return host.awaitJob(job, function () { throw new Error("continuation blew up"); });
  },
  trace: function () { return trace; },
};
"#;

/// Failure boundary for a wedged expectation, never evidence of success: every
/// wait below is released by an event the test itself signals, and a deadline
/// firing is always reported as a test failure.
const WEDGED: Duration = Duration::from_secs(30);

/// A distinct plugin id per test: exec jobs are keyed by it, and a VM dropping
/// at the end of one test reaps every pending job sharing its id.
fn serialization_vm(plugin_id: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source(plugin_id, SERIALIZATION_FIXTURE.to_string()).expect("load"))
}

type CallResult = Result<serde_json::Value, VmError>;

fn spawn_call(
    vm: &Arc<PluginVm>,
    method: &'static str,
    args: Vec<serde_json::Value>,
) -> Receiver<CallResult> {
    let (tx, rx) = mpsc::channel();
    let vm = vm.clone();
    thread::spawn(move || {
        let _ = tx.send(call_json::<serde_json::Value>(&vm, method, &args));
    });
    rx
}

fn recv_call(rx: &Receiver<CallResult>, what: &str) -> CallResult {
    rx.recv_timeout(WEDGED).unwrap_or_else(|_| {
        panic!("{what} never completed while another call was outstanding — the VM is serialized")
    })
}

#[test]
fn vm_serialization_slow_call_does_not_block_an_unrelated_call() {
    let entered = "vm_serialization_slow.entered";
    let release = "vm_serialization_slow.release";
    call_gate::reset(entered);
    call_gate::reset(release);

    let vm = serialization_vm("vm-serialization-slow");
    let slow_finished = Arc::new(AtomicBool::new(false));

    let slow_vm = vm.clone();
    let slow_flag = slow_finished.clone();
    let slow = thread::spawn(move || {
        let out = call_json::<serde_json::Value>(
            &slow_vm,
            "gated",
            &[json!(entered), json!(release), json!("slow")],
        );
        slow_flag.store(true, Ordering::SeqCst);
        out
    });
    assert!(
        call_gate::wait_opened(entered, WEDGED),
        "the slow call never reached its blocking point"
    );

    let fast = recv_call(
        &spawn_call(&vm, "fast", vec![json!("a")]),
        "the unrelated call",
    );

    assert_eq!(fast.expect("unrelated call failed"), json!(7));
    assert!(
        !call_gate::is_open(release),
        "the slow call was released before the unrelated call completed"
    );
    assert!(
        !slow_finished.load(Ordering::SeqCst),
        "the slow call finished on its own — it was never outstanding"
    );

    call_gate::open(release);
    let slow_out = slow
        .join()
        .expect("slow thread panicked")
        .expect("slow call failed");
    assert_eq!(slow_out["tag"], "slow");
    assert_eq!(slow_out["code"], 0);
}

#[test]
fn slow_plugin_does_not_block_an_unrelated_plugin() {
    let entered = "vm_serialization_cross_plugin.entered";
    let release = "vm_serialization_cross_plugin.release";
    call_gate::reset(entered);
    call_gate::reset(release);

    let slow_vm = serialization_vm("vm-cross-plugin-slow");
    let fast_vm = serialization_vm("vm-cross-plugin-fast");
    let slow = thread::spawn(move || {
        call_json::<serde_json::Value>(
            &slow_vm,
            "gated",
            &[json!(entered), json!(release), json!("slow")],
        )
    });
    assert!(
        call_gate::wait_opened(entered, WEDGED),
        "the slow plugin never reached its blocking point"
    );

    let fast = recv_call(
        &spawn_call(&fast_vm, "fast", vec![json!("other")]),
        "the unrelated plugin call",
    );
    assert_eq!(fast.expect("the unrelated plugin call failed"), json!(7));
    assert!(
        !call_gate::is_open(release),
        "the slow plugin was released before the unrelated call completed"
    );

    call_gate::open(release);
    slow.join()
        .expect("slow plugin thread panicked")
        .expect("slow plugin call failed");
}

#[test]
fn vm_async_call_refuses_after_its_bounded_event_wait() {
    let release = "vm_serialization_timeout.release";
    call_gate::reset(release);

    let vm = serialization_vm("vm-serialization-timeout");
    let result = call_json::<serde_json::Value>(&vm, "bounded", &[json!(release)]);

    call_gate::open(release);
    let error = result.expect_err("a wedged async job must be refused at the call bound");
    assert!(matches!(error, VmError::AsyncJobTimedOut));
    assert_eq!(error.to_string(), super::super::ASYNC_JOB_TIMEOUT_ERROR);
}

#[test]
fn vm_serialization_preserves_call_order_and_never_interleaves_frames() {
    let entered = "vm_serialization_order.entered";
    let release = "vm_serialization_order.release";
    call_gate::reset(entered);
    call_gate::reset(release);

    let vm = serialization_vm("vm-serialization-order");
    let slow_vm = vm.clone();
    let slow = thread::spawn(move || {
        call_json::<serde_json::Value>(
            &slow_vm,
            "gated",
            &[json!(entered), json!(release), json!("slow")],
        )
    });
    assert!(
        call_gate::wait_opened(entered, WEDGED),
        "the slow call never started"
    );

    for tag in ["one", "two", "three"] {
        let out = recv_call(
            &spawn_call(&vm, "fast", vec![json!(tag)]),
            "an interleaved call",
        );
        assert_eq!(out.expect("interleaved call failed"), json!(7));
    }

    call_gate::open(release);
    slow.join()
        .expect("slow thread panicked")
        .expect("slow call failed");

    let trace: Vec<String> = call_json(&vm, "trace", &[]).expect("trace");
    assert!(
        !trace.iter().any(|t| t.starts_with("INTERLEAVED")),
        "two calls were live in one context at once: {trace:?}"
    );
    assert_eq!(
        trace,
        vec![
            "slow".to_string(),
            "fast:one".to_string(),
            "fast:two".to_string(),
            "fast:three".to_string(),
            "slow:resume".to_string(),
        ],
        "the plugin observed its calls out of the order they were made"
    );
}

#[test]
fn vm_serialization_fast_path_is_unchanged_for_ordinary_returns() {
    let vm = fixture(
        r#"
        module.exports.default = {
            plain() { return { a: 1, b: [2, 3] }; },
            markerValue() { return { __ctAwait__: "not-a-marker" }; },
            markerShape() { return { __ctAwait__: { job: "x", k: "1", extra: true } }; },
            markerKeys() { return { __ctAwait__: { job: "x", k: "1" }, also: 1 }; },
            echo(s) { return s; },
        };
    "#,
    );
    assert_eq!(
        call_json::<serde_json::Value>(&vm, "plain", &[]).unwrap(),
        json!({ "a": 1, "b": [2, 3] })
    );
    assert_eq!(
        call_json::<serde_json::Value>(&vm, "markerValue", &[]).unwrap(),
        json!({ "__ctAwait__": "not-a-marker" })
    );
    assert_eq!(
        call_json::<serde_json::Value>(&vm, "markerShape", &[]).unwrap(),
        json!({ "__ctAwait__": { "job": "x", "k": "1", "extra": true } })
    );
    assert_eq!(
        call_json::<serde_json::Value>(&vm, "markerKeys", &[]).unwrap(),
        json!({ "__ctAwait__": { "job": "x", "k": "1" }, "also": 1 })
    );
    assert_eq!(
        call_json::<String>(&vm, "echo", &[json!("hi")]).unwrap(),
        "hi"
    );
}

#[test]
fn vm_serialization_error_in_flight_surfaces_and_leaves_the_vm_usable() {
    let entered = "vm_serialization_error.entered";
    let release = "vm_serialization_error.release";
    let second_entered = "vm_serialization_error.entered2";
    let second_release = "vm_serialization_error.release2";
    for gate in [entered, release, second_entered, second_release] {
        call_gate::reset(gate);
    }

    let vm = serialization_vm("vm-serialization-error");
    let boom_vm = vm.clone();
    let boom = thread::spawn(move || {
        call_json::<serde_json::Value>(&boom_vm, "boom", &[json!(entered), json!(release)])
    });
    assert!(
        call_gate::wait_opened(entered, WEDGED),
        "the failing call never started"
    );

    let missing = recv_call(&spawn_call(&vm, "nope", vec![]), "a failing unrelated call")
        .expect_err("a missing method must still error while an await is in flight");
    assert!(matches!(
        missing,
        VmError::Script(_) | VmError::MissingFunction(_)
    ));
    let during = recv_call(
        &spawn_call(&vm, "fast", vec![json!("during")]),
        "a good unrelated call",
    );
    assert_eq!(during.expect("good call failed"), json!(7));

    call_gate::open(release);
    let err = boom
        .join()
        .expect("boom thread panicked")
        .expect_err("a throwing continuation must surface as an error");
    assert!(matches!(&err, VmError::Script(_)), "unexpected: {err:?}");

    assert_eq!(
        call_json::<serde_json::Value>(&vm, "fast", &[json!("after")]).expect("vm unusable"),
        json!(7)
    );

    let again_vm = vm.clone();
    let again = thread::spawn(move || {
        call_json::<serde_json::Value>(
            &again_vm,
            "gated",
            &[json!(second_entered), json!(second_release), json!("again")],
        )
    });
    assert!(
        call_gate::wait_opened(second_entered, WEDGED),
        "the await mechanism did not survive the error"
    );
    call_gate::open(second_release);
    let out = again
        .join()
        .expect("thread panicked")
        .expect("await after an error failed");
    assert_eq!(out["tag"], "again");
}
