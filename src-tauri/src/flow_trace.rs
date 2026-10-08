use codeterm_types::trace::{Event, Sink};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc::SyncSender, OnceLock};

static SENDER: OnceLock<SyncSender<Event>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn tab_flow(service: &crate::service::ServiceContext, tab: &str) -> Option<String> {
    use tauri::Manager;
    if SENDER.get().is_none() {
        return None;
    }
    let mut bindings = service
        .plan_store
        .live_task_attempts_for_tab(tab)
        .into_iter()
        .filter(|binding| binding.plan_active && binding.attempt.retired_at.is_none());
    if let Some(binding) = bindings.next() {
        if bindings.next().is_none() {
            return Some(binding.attempt.id);
        }
    }
    service
        .app_handle
        .try_state::<crate::CronState>()
        .and_then(|state| state.lock().get_tab_owner(tab).map(|(_, run)| run.clone()))
}

pub(crate) fn envelope_flow(
    service: &crate::service::ServiceContext,
    target: &str,
    envelope: &crate::service::messenger::MessageEnvelope,
) -> Option<String> {
    if SENDER.get().is_none() {
        return None;
    }
    if let Some(id) = envelope.trace_id.as_ref() {
        return Some(id.clone());
    }
    match envelope.delivery_observer.as_ref() {
        Some(crate::service::messenger::DeliveryWriteObserver::Report {
            attempt_id: Some(id),
            ..
        }) => Some(id.clone()),
        _ => tab_flow(service, target)
            .or_else(|| tab_flow(service, &envelope.from_tab))
            .or_else(|| envelope.id.clone()),
    }
}

pub(crate) fn init() {
    if !crate::paths::is_dev_instance() {
        return;
    }
    let dir = crate::paths::data_dir().join("logs");
    let (sender, receiver) = std::sync::mpsc::sync_channel(4096);
    if SENDER.set(sender).is_err() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("flow-trace".into())
        .spawn(move || {
            let Ok(mut sink) = Sink::open(&dir, true) else {
                return;
            };
            for event in receiver {
                let dropped = DROPPED.swap(0, Ordering::Relaxed);
                if dropped > 0 {
                    let mut gap = Event::new(
                        &event.trace_id,
                        "trace.dropped",
                        &event.tab,
                        &event.provider,
                        "overflow",
                    );
                    gap.detail = format!("{dropped} events dropped from the bounded queue");
                    let _ = sink.append(&gap);
                }
                let _ = sink.append(&event);
            }
        });
}

pub(crate) fn record(
    id: &str,
    stage: &str,
    tab: &str,
    provider: &str,
    outcome: &str,
    related: Option<&str>,
) {
    if let Some(sender) = SENDER.get() {
        if sender
            .try_send(Event::new(id, stage, tab, provider, outcome).related(related))
            .is_err()
        {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) fn record_flow(
    id: &str,
    stage: &str,
    tab: &str,
    provider: &str,
    outcome: &str,
    flow: Option<&str>,
) {
    record(
        flow.unwrap_or(id),
        stage,
        tab,
        provider,
        outcome,
        flow.filter(|flow| *flow != id).map(|_| id),
    );
}
