use super::*;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

fn fixture(id: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source(id, "module.exports.default = { health() { return 'healthy'; }, loop() { while (true) {} } };".into()).unwrap())
}

#[test]
fn cancelled_call_exits_while_vm_dependency_remains_held() {
    let vm = fixture("vm-cancel-held");
    let healthy = fixture("vm-cancel-healthy");
    let held = vm.inner.lock();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    *vm.inner.waiting.lock() = Some(waiting_tx);
    let cancellation = crate::converter::ConversionCancellation::default();
    let caller_cancel = cancellation.clone();
    let caller_vm = vm.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let caller = std::thread::spawn(move || {
        result_tx
            .send(call_json_interruptible::<String>(
                &caller_vm,
                "health",
                &[],
                caller_cancel,
                Instant::now() + Duration::from_secs(5),
            ))
            .unwrap();
    });
    waiting_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    cancellation.cancel();
    let result = result_rx.recv_timeout(Duration::from_secs(1));
    let health = call_json::<String>(&healthy, "health", &[]);
    drop(held);
    caller.join().unwrap();
    assert!(matches!(result.unwrap(), Err(VmError::CallCancelled)));
    assert_eq!(health.unwrap(), "healthy");
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
}

#[test]
fn ordinary_calls_interrupt_unbounded_javascript_and_release_the_vm() {
    let vm = fixture("vm-default-deadline");
    // The call deadline alone bounds runaway frames; a short bound keeps the proof fast.
    super::super::host_exec::set_test_wait_bound("vm-default-deadline", Duration::from_millis(100));
    let worker_vm = vm.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        result_tx
            .send(call_json::<serde_json::Value>(&worker_vm, "loop", &[]))
            .unwrap();
    });
    let result = result_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("ordinary JS execution must be bounded");
    worker.join().unwrap();
    assert!(matches!(result, Err(VmError::AsyncJobTimedOut)));
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
}

#[test]
fn reload_cancels_a_queued_call_without_waiting_for_its_old_deadline() {
    let vm = fixture("vm-reload-cancel-held");
    let held = vm.inner.lock();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    *vm.inner.waiting.lock() = Some(waiting_tx);
    let caller_vm = vm.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let caller = std::thread::spawn(move || {
        result_tx
            .send(call_json::<String>(&caller_vm, "health", &[]))
            .unwrap();
    });
    waiting_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let paused = vm.admission.pause();
    let result = result_rx.recv_timeout(Duration::from_secs(1));
    drop(held);
    caller.join().unwrap();
    assert!(matches!(result.unwrap(), Err(VmError::CallCancelled)));
    assert!(matches!(
        call_json::<String>(&vm, "health", &[]),
        Err(VmError::AdmissionBusy)
    ));
    drop(paused);
    assert_eq!(call_json::<String>(&vm, "health", &[]).unwrap(), "healthy");
}
