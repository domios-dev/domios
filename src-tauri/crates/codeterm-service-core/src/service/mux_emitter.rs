use super::{MuxEmitCause, MuxEmitPending, MuxEmitStats, PublishedMuxState, ServiceContext};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};

#[derive(Default)]
pub(super) struct MuxEmitter(parking_lot::Mutex<Option<Emitter<ServiceContext>>>);

struct Pending<T> {
    latest: Option<T>,
    focuses: Vec<String>,
    snapshot_requested: bool,
    causes: u64,
}

struct Emitter<T> {
    wake: mpsc::SyncSender<()>,
    pending: Arc<parking_lot::Mutex<Pending<T>>>,
}

impl<T> Emitter<T> {
    fn channel() -> (Self, mpsc::Receiver<()>) {
        let (wake, receiver) = mpsc::sync_channel(1);
        let pending = Arc::new(parking_lot::Mutex::new(Pending {
            latest: None,
            focuses: Vec::new(),
            snapshot_requested: false,
            causes: 0,
        }));
        (Self { wake, pending }, receiver)
    }

    fn request(
        &self,
        state: T,
        focus: Option<String>,
        cause: Option<MuxEmitCause>,
    ) -> Option<(T, bool, Vec<String>, u64)> {
        let mut pending = self.pending.lock();
        pending.latest = Some(state);
        if let Some(cause) = cause {
            pending.causes |= cause.bit();
        }
        pending.snapshot_requested |= focus.is_none();
        pending.focuses.extend(focus);
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => None,
            Err(mpsc::TrySendError::Disconnected(())) => {
                let state = pending.latest.take()?;
                Some((
                    state,
                    std::mem::take(&mut pending.snapshot_requested),
                    std::mem::take(&mut pending.focuses),
                    std::mem::take(&mut pending.causes),
                ))
            }
        }
    }
}

impl MuxEmitter {
    pub(super) fn request(
        &self,
        service: &ServiceContext,
        focus: Option<String>,
        cause: Option<MuxEmitCause>,
    ) {
        let mut emitter = self.0.lock();
        if emitter.is_none() {
            let (sender, receiver) = Emitter::channel();
            let pending = sender.pending.clone();
            match std::thread::Builder::new()
                .name("mux-emit".into())
                .spawn(move || run(receiver, pending, emit))
            {
                Ok(_) => *emitter = Some(sender),
                Err(error) => {
                    log::error!("[events] mux emitter spawn failed: {error}");
                    drop(emitter);
                    emit(
                        service.clone(),
                        focus.is_none(),
                        focus.into_iter().collect(),
                        cause.map_or(0, MuxEmitCause::bit),
                    );
                    return;
                }
            }
        }
        if let Some(sender) = emitter.as_ref() {
            if let Some((state, snapshot_requested, focuses, causes)) =
                sender.request(service.clone(), focus, cause)
            {
                *emitter = None;
                drop(emitter);
                log::error!("[events] mux emitter disconnected");
                emit(state, snapshot_requested, focuses, causes);
            }
        }
    }
}

fn run<T>(
    receiver: mpsc::Receiver<()>,
    pending: Arc<parking_lot::Mutex<Pending<T>>>,
    mut emit: impl FnMut(T, bool, Vec<String>, u64),
) {
    while receiver.recv().is_ok() {
        let batch = {
            let mut pending = pending.lock();
            pending.latest.take().map(|state| {
                (
                    state,
                    std::mem::take(&mut pending.snapshot_requested),
                    std::mem::take(&mut pending.focuses),
                    std::mem::take(&mut pending.causes),
                )
            })
        };
        if let Some((state, snapshot_requested, focuses, causes)) = batch {
            emit(state, snapshot_requested, focuses, causes);
        }
    }
}

fn emit(service: ServiceContext, snapshot_requested: bool, focuses: Vec<String>, causes: u64) {
    if snapshot_requested {
        emit_snapshot(&service, causes);
    }
    for tab_id in focuses {
        service
            .ui
            .emit_native("tab-focused", serde_json::json!({"tab_id": tab_id}));
    }
}

pub(super) fn emit_snapshot(service: &ServiceContext, causes: u64) {
    let stats = service.ext::<Arc<MuxEmitStats>>();
    stats.pending_since_ms.store(0, Ordering::Relaxed);
    service
        .ext::<MuxEmitPending>()
        .0
        .store(false, Ordering::Release);
    let snapshot = super::status_reconciliation::snapshot_with_reconciled_status(
        &service.mux,
        &service.agent_tracker,
        &service.agent_registry,
    );
    let unchanged = match service
        .ext::<Arc<PublishedMuxState>>()
        .publish_change(snapshot.generation, &snapshot)
    {
        Ok(Some(signal)) => {
            service
                .events
                .publish_generic("mux-state-changed", signal.clone());
            service.events.broadcast("mux-state-changed", signal);
            false
        }
        Ok(None) => {
            stats.unchanged.fetch_add(1, Ordering::Relaxed);
            true
        }
        Err(error) => {
            log::error!("[mux-snapshot] publication failed: {error}");
            false
        }
    };
    if let Err(error) = super::session_persist::submit_session_snapshot_with_intent(
        service,
        &service.mux,
        crate::session::store::SaveIntent::PreserveDesk,
    ) {
        log::warn!("[session-persist] coalesced desk submission failed: {error}");
    }
    if !unchanged && !stats.frozen.load(Ordering::Acquire) {
        stats.fires.fetch_add(1, Ordering::Relaxed);
        stats.causes.fire(causes);
        service.ui.emit_native(
            "mux-state-changed",
            serde_json::json!({"generation": snapshot.generation}),
        );
    }
}

#[cfg(test)]
mod tests;
