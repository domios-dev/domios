use codeterm_types::trace::{Event, Sink};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc::SyncSender, OnceLock};

static SENDER: OnceLock<SyncSender<Event>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);

mod source;
mod watch;
#[cfg(test)]
pub(crate) use source::UnledgeredObservation;
pub(crate) use source::{link_rpc, PtyFlow, SourceCursor};
pub(crate) use watch::cancel as cancel_tab;
pub(crate) use watch::ensure as watch_pty;
#[cfg(test)]
pub(crate) use watch::{capture_window, register_source};

pub(crate) fn enabled() -> bool {
    #[cfg(test)]
    if test_capture::active() {
        return true;
    }
    SENDER.get().is_some()
}

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

pub fn record(
    id: &str,
    stage: &str,
    tab: &str,
    provider: &str,
    outcome: &str,
    related: Option<&str>,
) {
    if !enabled() {
        return;
    }
    let mut event = Event::new(id, stage, tab, provider, outcome).related(related);
    event.detail = related.unwrap_or_default().chars().take(240).collect();
    #[cfg(test)]
    if test_capture::record(event.clone()) {
        return;
    }
    if let Some(sender) = SENDER.get() {
        if sender.try_send(event).is_err() {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_capture {
    use super::Event;
    use std::cell::RefCell;

    thread_local! {
        static EVENTS: RefCell<Option<Vec<Event>>> = const { RefCell::new(None) };
    }

    pub(super) fn active() -> bool {
        EVENTS.with_borrow(|events| events.is_some())
    }

    pub(super) fn record(event: Event) -> bool {
        EVENTS.with_borrow_mut(|events| {
            if let Some(events) = events {
                events.push(event);
                true
            } else {
                false
            }
        })
    }

    pub(crate) fn capture(action: impl FnOnce()) -> Vec<Event> {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                EVENTS.with_borrow_mut(|events| *events = None);
            }
        }
        EVENTS.with_borrow_mut(|events| {
            assert!(events.is_none());
            *events = Some(Vec::new());
        });
        let _reset = Reset;
        action();
        EVENTS.with_borrow_mut(|events| events.take().unwrap())
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
