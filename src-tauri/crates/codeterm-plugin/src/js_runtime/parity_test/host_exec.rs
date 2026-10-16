use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde_json::{json, Value};

use super::super::host_api::PluginSandbox;
use super::super::vm::{call_json, PluginVm};

const FIXTURE: &str = r#"module.exports.default = {
    execute(opts) { return host.exec.async(opts, function (result) { return result; }); },
    fast() { return { ready: true }; }
};"#;
const DEADLINE: Duration = Duration::from_secs(30);
const CHILD_ADDRESS: &str = "CODETERM_EXEC_PARITY_CHILD_ADDRESS";

#[test]
fn async_exec_native_js_result_parity() {
    let _guard = super::super::PROCESS_TEST_LOCK.lock();
    let vm = PluginVm::from_source("exec-result-parity", FIXTURE.to_string()).unwrap();
    #[cfg(windows)]
    let opts = json!({
        "bin": "powershell",
        "args": ["-NoProfile", "-Command", "[Console]::Out.Write('fixture'); [Console]::Error.Write('diagnostic'); exit 7"],
        "timeoutMs": 4000
    });
    #[cfg(not(windows))]
    let opts = json!({
        "bin": "sh", "args": ["-c", "printf fixture; printf diagnostic >&2; exit 7"],
        "timeoutMs": 4000
    });
    let native: Value =
        serde_json::from_str(&super::super::host_exec::run(&opts.to_string())).unwrap();
    assert_eq!(
        native,
        json!({ "code": 7, "stdout": "fixture", "stderr": "diagnostic" })
    );
    let mut js: Value = call_json(&vm, "execute", &[opts]).unwrap();
    assert_eq!(
        js.as_object_mut().unwrap().remove("done"),
        Some(json!(true))
    );
    assert_eq!(js, native);
}

#[test]
fn async_exec_confined_denial_matches_sync() {
    let sandbox = Arc::new(PluginSandbox::restricted(vec![], vec![]));
    let source = format!(
        "{}; module.exports.default.sync = function (opts) {{ return JSON.parse(host.exec(JSON.stringify(opts))); }};",
        FIXTURE
    );
    let vm = PluginVm::from_source_sandboxed("exec-denial-parity", source, None, sandbox).unwrap();
    let opts = json!({ "bin": "ungranted-fixture" });
    let sync: Value = call_json(&vm, "sync", &[opts.clone()]).unwrap();
    let asynchronous: Value = call_json(&vm, "execute", &[opts]).unwrap();
    assert_eq!(sync, json!({ "error": "exec denied: ungranted-fixture" }));
    assert_eq!(asynchronous, sync);
}

#[test]
fn async_exec_child_fixture() {
    let Ok(address) = std::env::var(CHILD_ADDRESS) else {
        return;
    };
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.write_all(b"ready").unwrap();
    let mut release = [0];
    stream.read_exact(&mut release).unwrap();
    assert_eq!(release, [1]);
    print!("child-released");
}

#[test]
fn async_exec_slow_child_releases_vm_for_second_hook() {
    let _guard = super::super::PROCESS_TEST_LOCK.lock();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (connected_tx, connected_rx) = mpsc::channel();
    let accept = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        stream.set_write_timeout(Some(DEADLINE)).unwrap();
        let mut ready = [0; 5];
        stream.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"ready");
        connected_tx.send(stream).unwrap();
    });
    let vm =
        Arc::new(PluginVm::from_source("exec-child-concurrency", FIXTURE.to_string()).unwrap());
    super::super::host_exec::set_test_wait_bound("exec-child-concurrency", DEADLINE);
    let opts = json!({
        "bin": std::env::current_exe().unwrap(),
        "args": ["--exact", "js_runtime::parity_test::host_exec::async_exec_child_fixture", "--nocapture"],
        "env": { (CHILD_ADDRESS): address.to_string() },
        "timeoutMs": DEADLINE.as_millis() as u64
    });
    let slow_vm = vm.clone();
    let slow = std::thread::spawn(move || call_json::<Value>(&slow_vm, "execute", &[opts]));
    let mut stream = connected_rx
        .recv_timeout(DEADLINE)
        .expect("child handshake");
    let (fast_tx, fast_rx) = mpsc::channel();
    let fast_vm = vm.clone();
    let fast = std::thread::spawn(move || {
        fast_tx
            .send(call_json::<Value>(&fast_vm, "fast", &[]))
            .unwrap();
    });
    let result = fast_rx.recv_timeout(DEADLINE);
    let outstanding = !slow.is_finished();
    stream.write_all(&[1]).unwrap();
    let child = slow.join().unwrap().unwrap();
    accept.join().unwrap();
    fast.join().unwrap();
    assert_eq!(result.unwrap().unwrap(), json!({ "ready": true }));
    assert!(
        outstanding,
        "second hook must finish before the child exits"
    );
    assert_eq!(child["code"], 0);
    assert!(child["stdout"].as_str().unwrap().contains("child-released"));
}
