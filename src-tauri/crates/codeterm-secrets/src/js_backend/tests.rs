use super::*;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

/// Failure bound for a wedged expectation; every wait is released by a signal.
const WEDGED: Duration = Duration::from_secs(30);

struct ParkedVault {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl PluginCalls for ParkedVault {
    fn has_method(&self, _: &str) -> bool {
        true
    }

    fn call(&self, method: &str, _: &[serde_json::Value]) -> Result<serde_json::Value, VmError> {
        assert_eq!(method, "secretGetItem");
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        self.release
            .lock()
            .unwrap()
            .recv_timeout(WEDGED)
            .expect("the parked lookup was never released");
        Ok(json!({ "ok": { "item_id": "id", "name": "token", "value": "v", "notes": null } }))
    }
}

#[test]
fn a_parked_vault_lookup_leaves_the_async_runtime_serving_other_tasks() {
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();
    let backend = JsSecretBackend::new(
        Arc::new(ParkedVault {
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(release_rx),
        }),
        "parked-vault",
        false,
    );

    let runtime = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let lookup = tokio::spawn(async move { backend.get("token").await });
            entered_rx.await.expect("the lookup reached the vault");
            progress_tx.send(()).expect("test thread listening");
            lookup.await.expect("lookup task")
        })
    });

    progress_rx
        .recv_timeout(WEDGED)
        .expect("the runtime stalled behind a parked vault lookup");
    release_tx.send(()).expect("lookup parked");
    let secret = runtime
        .join()
        .expect("runtime thread")
        .expect("lookup result");
    assert_eq!(secret.value.expose_secret(), "v");
}
