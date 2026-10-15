use std::sync::mpsc;
use std::time::Duration;

pub(crate) struct Shutdown(mpsc::Receiver<()>);

pub(crate) fn begin() -> Shutdown {
    let sidecars = crate::service::structured_endpoint::take_rpc();
    let cleanups: Vec<_> = sidecars
        .iter()
        .map(|sidecar| {
            sidecar.client.close("Daemon exiting");
            sidecar.process.cleanup()
        })
        .collect();
    let (done, wait) = mpsc::channel();
    tauri::async_runtime::spawn(async move {
        let results =
            futures_util::future::join_all(cleanups.into_iter().map(|cleanup| cleanup.finish()))
                .await;
        for error in results.into_iter().filter_map(Result::err) {
            log::warn!("[shutdown] Codex sidecar: {error}");
        }
        drop(sidecars);
        let _ = done.send(());
    });
    Shutdown(wait)
}

impl Shutdown {
    pub(crate) fn wait(self) {
        if self.0.recv_timeout(Duration::from_secs(6)).is_err() {
            log::warn!("[shutdown] Codex sidecar cleanup deadline");
        }
    }
}
