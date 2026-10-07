use super::*;

pub(super) fn start_backend_reader(
    svc: &ServiceContext,
    pty_id: &str,
) -> Result<ReaderReadiness, LaunchFailure> {
    let svc_arc = std::sync::Arc::new(svc.clone());
    let prepared = crate::service::pty_lifecycle::prepare_backend_reader(&svc_arc, pty_id)
        .map_err(LaunchFailure::ReaderSetup)?;

    let Some(prepared) = prepared else {
        return Ok(ReaderReadiness::Ready);
    };
    let reader_pty_id = pty_id.to_string();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (closed_tx, closed_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = ready_tx.send(Err(LaunchFailure::ReaderRuntime(error.to_string())));
                return;
            }
        };
        runtime.block_on(async {
            let reader = crate::service::pty_lifecycle::spawn_prepared_backend_reader(
                &svc_arc,
                &reader_pty_id,
                prepared,
                true,
            );
            let _ = ready_tx.send(Ok(()));
            let _ = reader.await;
            let _ = closed_tx.send(());
        });
    });
    Ok(ReaderReadiness::Pending {
        startup: ready_rx,
        closed: closed_rx,
    })
}
