use super::*;
use crate::converter::ConversionCancellation;
use crate::js_runtime::{
    call_gate,
    vm::{call_json, call_json_interruptible, PluginVm},
};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WEDGED: Duration = Duration::from_secs(1);
const SOURCE: &str = r#"
let lockId = null;
let rememberedJob = null;
let callbackEntries = 0;
module.exports = {
  health() { return "healthy"; },
  remembered(request) { rememberedJob = JSON.parse(host.ioStart(JSON.stringify(request))).jobId; return host.awaitJob(rememberedJob, function() { return "owned"; }); },
  steal() { return host.awaitJob(rememberedJob, function() { return "stolen"; }); },
  range(path) { return host.io.run((function*() { try { yield* host.io.fs.readFileHead(path, 2097152); } catch(error) { return error instanceof RangeError; } return false; })()); },
  tail(path, bytes) { return host.io.run((function*() { try { return yield* host.io.fs.readFileTail(path, bytes); } catch(error) { return {range:error instanceof RangeError}; } })()); },
  query(db, sql) { return host.io({op:"sqliteQuery",db:db,sql:sql,params:[]}, function(rows) { return rows; }); },
  nullableQuery(db, sql) { return host.io({op:"sqliteQuery",db:db,sql:sql,params:[],nullable:true}, function(rows) { return rows; }); },
  generatorCallback(path) { return host.io({op:"readFile",path:path}, function*(text) {
    callbackEntries++;
    yield* host.io.fs.writeFile(path + ".result", text + String(callbackEntries));
    return callbackEntries;
  }); },
  nestedGenerator(path) { return host.io.run((function*() { return (function*() { return yield* host.io.fs.readFile(path); })(); })()); },
  legacy(path) { return host.readFile(path); },
  pause(request) { return host.io(request, function() { return "resumed"; }); },
  read(path) { return host.io.run((function*() { return yield* host.io.fs.readFile(path); })()); },
  write(path) { return host.io.run((function*() { return yield* host.io.fs.writeFile(path, "written-once"); })()); },
  unlock(id) { return host.io.run((function*() { return yield* host.io.fs.unlockFile(id); })()); },
  locked(path, request, target) { return host.io.run((function*() {
    lockId = yield* host.io.fs.lockFile(path);
    yield request;
    return yield* host.io.fs.writeFile(target, "must-not-run");
  })()); },
  lockId() { return lockId; },
  unawaited(request) { host.ioStart(JSON.stringify(request)); host.testGateBlock(request.entered); return "not-awaited"; },
};
"#;

struct Gates {
    entered: String,
    release: String,
    completed: String,
}
impl Gates {
    fn new(name: &str) -> Self {
        let gates = Self {
            entered: format!("{name}.entered"),
            release: format!("{name}.release"),
            completed: format!("{name}.completed"),
        };
        for name in [&gates.entered, &gates.release, &gates.completed] {
            call_gate::reset(name);
        }
        gates
    }
    fn request(&self) -> Value {
        json!({"op":"testGate","entered":self.entered,"release":self.release,"completed":self.completed})
    }
    fn entered(&self) {
        assert!(
            call_gate::wait_opened(&self.entered, WEDGED),
            "the native I/O job must own the dependency"
        );
    }
    fn finish(&self) {
        call_gate::open(&self.release);
        assert!(
            call_gate::wait_opened(&self.completed, WEDGED),
            "native I/O resources must finish"
        );
    }
}
impl Drop for Gates {
    fn drop(&mut self) {
        call_gate::open(&self.release);
    }
}

fn vm(id: &str, roots: Option<&std::path::Path>) -> Arc<PluginVm> {
    let sandbox = match roots {
        Some(root) => {
            PluginSandbox::restricted(vec![root.into()], vec![root.into()]).with_identity(id)
        }
        None => PluginSandbox::unrestricted().with_identity(id),
    };
    let manifest =
        json!({"id":id,"version":"0.0.0","displayName":"Fixture","hostIoJobs":true}).to_string();
    Arc::new(
        PluginVm::from_source_sandboxed(id, SOURCE.into(), Some(manifest), Arc::new(sandbox))
            .unwrap(),
    )
}

#[test]
fn held_native_job_leaves_the_same_vm_free_for_pure_calls() {
    let vm = vm("io-held-native", None);
    let gates = Gates::new("io-held-native");
    let caller_vm = vm.clone();
    let request = gates.request();
    let caller = std::thread::spawn(move || call_json::<String>(&caller_vm, "pause", &[request]));
    gates.entered();
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
    gates.finish();
    assert_eq!(caller.join().unwrap().unwrap(), "resumed");
}

#[test]
fn jobs_only_contract_refuses_legacy_callbacks_and_preserves_confined_io() {
    let root = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    let owned = root.path().join("owned.txt");
    let denied = foreign.path().join("denied.txt");
    std::fs::write(&owned, "owned").unwrap();
    std::fs::write(&denied, "foreign").unwrap();
    let vm = vm("io-confinement", Some(root.path()));
    assert!(
        matches!(call_json::<Value>(&vm, "legacy", &[json!(owned)]), Err(VmError::Script(message)) if message.contains("requires a host.io job"))
    );
    assert_eq!(
        call_json::<Value>(&vm, "read", &[json!(owned)]).unwrap(),
        "owned"
    );
    assert_eq!(
        call_json::<Value>(&vm, "read", &[json!(denied)]).unwrap(),
        Value::Null
    );
    assert_eq!(
        call_json::<Value>(&vm, "write", &[json!(denied)]).unwrap(),
        false
    );
    assert_eq!(std::fs::read_to_string(denied).unwrap(), "foreign");
}

#[test]
fn observation_authority_reaches_the_job_owner_without_losing_read_access() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("observed.txt");
    std::fs::write(&path, "retained").unwrap();
    let vm = vm("io-observation", Some(root.path()));
    let _observation = codeterm_decision::physical::RequestScope::observation_only(
        Instant::now() + Duration::from_secs(2),
    );
    assert_eq!(
        call_json::<Value>(&vm, "read", &[json!(path)]).unwrap(),
        "retained"
    );
    assert_eq!(
        call_json::<Value>(&vm, "write", &[json!(path)]).unwrap(),
        false
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "retained");
}

#[test]
fn cancelled_operation_never_admits_its_first_effect() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("never-created.txt");
    let vm = vm("io-cancel-before-effect", Some(root.path()));
    let cancellation = ConversionCancellation::default();
    cancellation.cancel();
    assert!(matches!(
        call_json_interruptible::<Value>(
            &vm,
            "write",
            &[json!(path)],
            cancellation,
            Instant::now() + Duration::from_secs(5)
        ),
        Err(VmError::CallCancelled)
    ));
    assert!(!path.exists());
}

#[test]
fn started_job_retains_call_locks_after_cancellation_and_never_replays_a_later_effect() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("held.lock");
    let target = root.path().join("never-written.txt");
    let vm = vm("io-resource-retention", Some(root.path()));
    let gates = Gates::new("io-resource-retention");
    let cancellation = ConversionCancellation::default();
    let caller_cancel = cancellation.clone();
    let caller_vm = vm.clone();
    let arguments = vec![json!(path), gates.request(), json!(target)];
    let (result_tx, result_rx) = mpsc::channel();
    let caller = std::thread::spawn(move || {
        result_tx
            .send(call_json_interruptible::<Value>(
                &caller_vm,
                "locked",
                &arguments,
                caller_cancel,
                Instant::now() + Duration::from_secs(5),
            ))
            .unwrap()
    });
    gates.entered();
    let lock_id: String = call_json(&vm, "lockId", &[]).unwrap();
    assert_eq!(
        call_json::<Value>(&vm, "unlock", &[json!(lock_id)]).unwrap(),
        false,
        "a separate invocation cannot unlock another call's file"
    );
    cancellation.cancel();
    let result = result_rx.recv_timeout(WEDGED);
    let observer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let retained = observer.try_lock().is_err();
    gates.finish();
    caller.join().unwrap();
    assert!(matches!(result.unwrap(), Err(VmError::IoUnconfirmed)));
    assert!(
        retained,
        "started native work must retain its invocation resources"
    );
    observer.try_lock().unwrap();
    observer.unlock().unwrap();
    assert!(!target.exists());
}

#[test]
fn reload_discards_late_native_completion_without_resuming_the_old_provider() {
    let vm = vm("io-late-reload", None);
    let gates = Gates::new("io-late-reload");
    let caller_vm = vm.clone();
    let request = gates.request();
    let (result_tx, result_rx) = mpsc::channel();
    let caller = std::thread::spawn(move || {
        result_tx
            .send(call_json::<String>(&caller_vm, "pause", &[request]))
            .unwrap()
    });
    gates.entered();
    vm.reload("module.exports = {health(){return 'reloaded';}};".into())
        .unwrap();
    assert!(matches!(
        result_rx.recv_timeout(WEDGED).unwrap(),
        Err(VmError::CallCancelled)
    ));
    gates.finish();
    caller.join().unwrap();
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "reloaded");
}

#[test]
fn current_thread_runtime_refuses_synchronous_dispatch_before_starting_native_work() {
    let vm = vm("io-current-thread", None);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(matches!(
            call_json::<String>(&vm, "health", &[]),
            Err(VmError::OffRuntimeRequired)
        ));
    });
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
}

#[tokio::test]
async fn current_thread_runtime_allows_owned_blocking_dispatch() {
    let vm = vm("io-owned-worker", None);
    let worker_vm = vm.clone();
    let value = crate::js_runtime::vm::spawn_blocking(move || {
        call_json::<String>(&worker_vm, "health", &[])
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(value, "healthy");
    assert!(matches!(
        call_json::<String>(&vm, "health", &[]),
        Err(VmError::OffRuntimeRequired)
    ));
}

#[test]
fn bounded_tail_reads_keep_their_own_limit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let contents = "x".repeat(4 * 1024 * 1024);
    std::fs::write(&path, &contents).unwrap();
    let vm = vm("io-tail-bound", Some(root.path()));
    assert_eq!(
        call_json::<String>(&vm, "tail", &[json!(path), json!(contents.len())]).unwrap(),
        contents
    );
    assert_eq!(
        call_json::<Value>(&vm, "tail", &[json!(path), json!(contents.len() + 1)]).unwrap(),
        json!({"range":true})
    );
    assert_eq!(
        call_json::<Value>(&vm, "range", &[json!(path)]).unwrap(),
        json!(true)
    );
    assert!(call_json::<Value>(&vm, "read", &[json!(path)]).is_err());
}

#[test]
fn asynchronous_dispatch_without_a_runtime_refuses_before_admission() {
    let vm = vm("io-no-runtime", None);
    let result = futures_executor::block_on(crate::js_runtime::vm::call_json_async::<String>(
        vm.clone(),
        "health",
        Vec::new(),
    ));
    assert!(matches!(result, Err(VmError::RuntimeInit(_))));
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
}

#[test]
fn one_worker_runtime_keeps_scheduling_while_a_native_dependency_is_held() {
    let vm = vm("io-one-worker", None);
    let gates = Gates::new("io-one-worker");
    let request = gates.request();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let caller_vm = vm.clone();
        let caller =
            tokio::spawn(async move { call_json::<String>(&caller_vm, "pause", &[request]) });
        gates.entered();
        let healthy_vm = vm.clone();
        let health = tokio::spawn(async move { call_json::<String>(&healthy_vm, "health", &[]) });
        let result = tokio::time::timeout(WEDGED, health).await;
        gates.finish();
        assert_eq!(result.unwrap().unwrap().unwrap(), "healthy");
        assert_eq!(caller.await.unwrap().unwrap(), "resumed");
    });
}

#[cfg(unix)]
#[test]
fn job_owner_refuses_a_symlink_outside_the_read_root() {
    let root = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    let secret = foreign.path().join("secret.txt");
    std::fs::write(&secret, "foreign").unwrap();
    let link = root.path().join("link.txt");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let vm = vm("io-symlink", Some(root.path()));
    assert_eq!(
        call_json::<Value>(&vm, "read", &[json!(link)]).unwrap(),
        Value::Null
    );
}

#[test]
fn cancellation_does_not_release_native_capacity_until_started_jobs_exit() {
    let vm = vm("io-capacity", None);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("healthy.txt");
    std::fs::write(&path, "healthy").unwrap();
    let mut running = Vec::new();
    for index in 0..4 {
        let gates = Gates::new(&format!("io-capacity-{index}"));
        let request = gates.request();
        let cancellation = ConversionCancellation::default();
        let caller_cancel = cancellation.clone();
        let caller_vm = vm.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let caller = std::thread::spawn(move || {
            result_tx
                .send(call_json_interruptible::<String>(
                    &caller_vm,
                    "pause",
                    &[request],
                    caller_cancel,
                    Instant::now() + Duration::from_secs(5),
                ))
                .unwrap()
        });
        gates.entered();
        running.push((gates, cancellation, result_rx, caller));
    }
    for (_, cancellation, _, _) in &running {
        cancellation.cancel();
    }
    for (_, _, result, _) in &running {
        assert!(matches!(
            result.recv_timeout(WEDGED).unwrap(),
            Err(VmError::CallCancelled)
        ));
    }
    let queued_vm = vm.clone();
    let queued_path = path.clone();
    let queued =
        std::thread::spawn(move || call_json::<Value>(&queued_vm, "read", &[json!(queued_path)]));
    assert!(
        admission::wait_queued("io-capacity", 1, WEDGED),
        "a read beyond retained capacity must queue, not run or fail"
    );
    let other = self::vm("io-capacity-independent", None);
    assert_eq!(
        call_json::<Value>(&other, "read", &[json!(path)]).unwrap(),
        "healthy"
    );
    for (gates, _, _, caller) in running {
        gates.finish();
        caller.join().unwrap();
    }
    assert_eq!(queued.join().unwrap().unwrap(), "healthy");
}

#[test]
fn saturated_capacity_queues_concurrent_status_reads_and_completes_every_one() {
    let plugin = "io-saturation";
    let vm = vm(plugin, None);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("status.txt");
    std::fs::write(&path, "healthy").unwrap();
    let slow = (0..4)
        .map(|index| {
            let gates = Gates::new(&format!("io-saturation-slow-{index}"));
            let request = gates.request();
            let caller_vm = vm.clone();
            let caller =
                std::thread::spawn(move || call_json::<String>(&caller_vm, "pause", &[request]));
            gates.entered();
            (gates, caller)
        })
        .collect::<Vec<_>>();
    let readers = (0..8)
        .map(|_| {
            let reader_vm = vm.clone();
            let reader_path = path.clone();
            std::thread::spawn(move || {
                call_json::<Value>(&reader_vm, "read", &[json!(reader_path)])
            })
        })
        .collect::<Vec<_>>();
    assert!(
        admission::wait_queued(plugin, 8, WEDGED),
        "every status read must wait for capacity instead of failing"
    );
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
    for (gates, caller) in slow {
        gates.finish();
        assert_eq!(caller.join().unwrap().unwrap(), "resumed");
    }
    for reader in readers {
        assert_eq!(reader.join().unwrap().unwrap(), "healthy");
    }
}

#[test]
fn cancelled_queued_reads_free_queue_and_job_slots_while_workers_stay_held() {
    let plugin = "io-queue-cancel";
    let vm = vm(plugin, None);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("status.txt");
    std::fs::write(&path, "healthy").unwrap();
    let held = (0..4)
        .map(|index| {
            let gates = Gates::new(&format!("io-queue-cancel-held-{index}"));
            let request = gates.request();
            let caller_vm = vm.clone();
            let caller = std::thread::spawn(move || {
                call_json_interruptible::<String>(
                    &caller_vm,
                    "pause",
                    &[request],
                    ConversionCancellation::default(),
                    Instant::now() + Duration::from_secs(60),
                )
            });
            gates.entered();
            (gates, caller)
        })
        .collect::<Vec<_>>();
    let held_jobs = crate::js_runtime::host_exec::plugin_job_count(plugin);
    // More cycles than the queue bound: a retained entry or job record would surface.
    for _ in 0..40 {
        let cancellation = ConversionCancellation::default();
        let caller_cancel = cancellation.clone();
        let caller_vm = vm.clone();
        let caller_path = path.clone();
        let caller = std::thread::spawn(move || {
            call_json_interruptible::<Value>(
                &caller_vm,
                "read",
                &[json!(caller_path)],
                caller_cancel,
                Instant::now() + Duration::from_secs(60),
            )
        });
        assert!(admission::wait_queued(plugin, 1, WEDGED));
        cancellation.cancel();
        assert!(matches!(
            caller.join().unwrap(),
            Err(VmError::CallCancelled)
        ));
        assert_eq!(admission::queued(plugin), 0);
        assert_eq!(
            crate::js_runtime::host_exec::plugin_job_count(plugin),
            held_jobs
        );
    }
    for (gates, caller) in held {
        gates.finish();
        assert_eq!(caller.join().unwrap().unwrap(), "resumed");
    }
    assert_eq!(
        call_json::<Value>(&vm, "read", &[json!(path)]).unwrap(),
        "healthy"
    );
}

#[test]
fn unawaited_native_jobs_are_refused_without_reentering_their_caller() {
    let vm = vm("io-unawaited", None);
    let gates = Gates::new("io-unawaited");
    let result = call_json::<Value>(&vm, "unawaited", &[gates.request()]);
    gates.finish();
    assert!(matches!(result, Err(VmError::Script(message)) if message.contains("was not awaited")));
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
}

#[test]
fn another_invocation_cannot_consume_a_pending_host_io_result() {
    let vm = vm("io-exact-owner", None);
    let gates = Gates::new("io-exact-owner");
    let request = gates.request();
    let caller_vm = vm.clone();
    let caller =
        std::thread::spawn(move || call_json::<String>(&caller_vm, "remembered", &[request]));
    gates.entered();
    let stolen = call_json::<Value>(&vm, "steal", &[]);
    gates.finish();
    assert!(
        matches!(stolen, Err(VmError::Script(message)) if message.contains("foreign, unavailable or denied"))
    );
    assert_eq!(caller.join().unwrap().unwrap(), "owned");
}

#[test]
fn owner_enforces_file_bounds_and_preserves_read_only_sqlite() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("fixture.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("CREATE TABLE entries(value TEXT); INSERT INTO entries VALUES('retained');")
        .unwrap();
    let vm = vm("io-bounds-sqlite", Some(root.path()));
    assert_eq!(
        call_json::<Value>(&vm, "range", &[json!(db)]).unwrap(),
        true
    );
    let rows: Value = call_json(
        &vm,
        "query",
        &[json!(db), json!("SELECT value FROM entries")],
    )
    .unwrap();
    assert_eq!(rows, json!([{"value":"retained"}]));
    assert_eq!(
        call_json::<Value>(&vm, "query", &[json!(db), json!("DELETE FROM entries")]).unwrap(),
        json!([])
    );
    assert_eq!(
        connection
            .query_row("SELECT value FROM entries", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "retained"
    );
}

#[test]
fn generator_callbacks_resume_once_and_nested_programs_finish() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("callback.txt");
    std::fs::write(&path, "once-").unwrap();
    let vm = vm("io-generator-callback", Some(root.path()));
    assert_eq!(
        call_json::<Value>(&vm, "generatorCallback", &[json!(path)]).unwrap(),
        json!(1)
    );
    assert_eq!(
        std::fs::read_to_string(path.with_extension("txt.result")).unwrap(),
        "once-1"
    );
    assert_eq!(
        call_json::<Value>(&vm, "nestedGenerator", &[json!(path)]).unwrap(),
        json!("once-")
    );
}

#[test]
fn nullable_sqlite_preserves_unknown_without_authorizing_writes() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("nullable.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("CREATE TABLE entries(value TEXT); INSERT INTO entries VALUES('retained');")
        .unwrap();
    let vm = vm("io-nullable-sqlite", Some(root.path()));
    assert_eq!(
        call_json::<Value>(
            &vm,
            "nullableQuery",
            &[json!(db), json!("SELECT value FROM entries WHERE 0")]
        )
        .unwrap(),
        json!([])
    );
    assert_eq!(
        call_json::<Value>(
            &vm,
            "nullableQuery",
            &[json!(db), json!("DELETE FROM entries")]
        )
        .unwrap(),
        Value::Null
    );
    assert_eq!(
        connection
            .query_row("SELECT value FROM entries", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "retained"
    );
}
