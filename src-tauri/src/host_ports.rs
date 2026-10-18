#[cfg(test)]
use std::time::Instant;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
    time::Duration,
};

use codeterm_service_core::ports::host::{Shell, ShellError, UiEvents, WebviewRequest};
use serde_json::Value;
use tauri::{Emitter, Manager};

#[cfg(test)]
const MAX_PENDING_NATIVE_CALLS: usize = 64;

const MAX_QUEUED_MAIN_THREAD_CALLS: usize = 64;
const MAIN_THREAD_ACK_DEADLINE: Duration = Duration::from_secs(10);
const MAIN_THREAD_QUEUED: u8 = 0;
const MAIN_THREAD_RUNNING: u8 = 1;
const MAIN_THREAD_CANCELLED: u8 = 2;
const MAIN_THREAD_COMPLETED: u8 = 3;
static QUEUED_MAIN_THREAD_CALLS: AtomicUsize = AtomicUsize::new(0);

struct MainThreadQueuePermit;

impl MainThreadQueuePermit {
    fn acquire() -> Result<Self, ShellError> {
        QUEUED_MAIN_THREAD_CALLS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                (pending < MAX_QUEUED_MAIN_THREAD_CALLS).then_some(pending + 1)
            })
            .map_err(|_| ShellError::Failed("Native main-thread queue is full".into()))?;
        Ok(Self)
    }
}

impl Drop for MainThreadQueuePermit {
    fn drop(&mut self) {
        QUEUED_MAIN_THREAD_CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct NativeConnection {
    server_epoch: String,
    generation: u64,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct NativeRequestId(u64);

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeCallOutcome {
    CancelledBeforeDispatch,
    DisconnectedBeforeDispatch,
    TimedOutBeforeDispatch,
    Uncertain,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeReplyDisposition {
    Accepted,
    IgnoredStale,
    IgnoredBeforeDispatch,
    Expired(NativeCallOutcome),
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeCallError {
    NotAttached,
    PendingLimit,
    RequestIdsExhausted,
    DeadlineOverflow,
    UnknownRequest,
    StaleConnection,
    AlreadyDispatched,
    Expired,
}

#[cfg(test)]
struct PendingNativeCall {
    connection: NativeConnection,
    deadline: Instant,
    dispatched: bool,
}

#[cfg(test)]
struct NativeCallRetirement {
    id: NativeRequestId,
    outcome: NativeCallOutcome,
}

#[cfg(test)]
struct NativeCallRegistry {
    connection: Option<NativeConnection>,
    next_id: Option<u64>,
    pending: std::collections::HashMap<NativeRequestId, PendingNativeCall>,
}

#[cfg(test)]
impl Default for NativeCallRegistry {
    fn default() -> Self {
        Self {
            connection: None,
            next_id: Some(1),
            pending: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
impl NativeCallRegistry {
    fn attach(&mut self, connection: NativeConnection) -> Vec<NativeCallRetirement> {
        if self.connection.as_ref() == Some(&connection) {
            return Vec::new();
        }
        self.connection = Some(connection);
        self.retire_pending()
    }

    fn detach(&mut self, connection: &NativeConnection) -> Vec<NativeCallRetirement> {
        if self.connection.as_ref() != Some(connection) {
            return Vec::new();
        }
        self.connection = None;
        self.retire_pending()
    }

    fn begin(
        &mut self,
        connection: &NativeConnection,
        now: Instant,
        budget: Duration,
    ) -> Result<NativeRequestId, NativeCallError> {
        if self.connection.as_ref() != Some(connection) {
            return Err(NativeCallError::NotAttached);
        }
        if self.pending.len() >= MAX_PENDING_NATIVE_CALLS {
            return Err(NativeCallError::PendingLimit);
        }
        if budget.is_zero() {
            return Err(NativeCallError::Expired);
        }
        let deadline = now
            .checked_add(budget)
            .ok_or(NativeCallError::DeadlineOverflow)?;
        let id = NativeRequestId(self.next_id.ok_or(NativeCallError::RequestIdsExhausted)?);
        self.next_id = id.0.checked_add(1);
        self.pending.insert(
            id,
            PendingNativeCall {
                connection: connection.clone(),
                deadline,
                dispatched: false,
            },
        );
        Ok(id)
    }

    fn mark_dispatched(
        &mut self,
        connection: &NativeConnection,
        id: NativeRequestId,
        now: Instant,
    ) -> Result<(), NativeCallError> {
        self.require_current(connection)?;
        let call = self
            .pending
            .get(&id)
            .ok_or(NativeCallError::UnknownRequest)?;
        if call.connection != *connection {
            return Err(NativeCallError::StaleConnection);
        }
        let expired = call.deadline <= now;
        let dispatched = call.dispatched;
        if expired {
            self.pending.remove(&id);
            return Err(NativeCallError::Expired);
        }
        if dispatched {
            return Err(NativeCallError::AlreadyDispatched);
        }
        let call = self
            .pending
            .get_mut(&id)
            .ok_or(NativeCallError::UnknownRequest)?;
        call.dispatched = true;
        Ok(())
    }

    fn cancel(
        &mut self,
        connection: &NativeConnection,
        id: NativeRequestId,
    ) -> Result<NativeCallOutcome, NativeCallError> {
        self.require_current(connection)?;
        let call = self
            .pending
            .get(&id)
            .ok_or(NativeCallError::UnknownRequest)?;
        if call.connection != *connection {
            return Err(NativeCallError::StaleConnection);
        }
        let dispatched = call.dispatched;
        self.pending.remove(&id);
        Ok(if dispatched {
            NativeCallOutcome::Uncertain
        } else {
            NativeCallOutcome::CancelledBeforeDispatch
        })
    }

    fn accept_reply(
        &mut self,
        connection: &NativeConnection,
        id: NativeRequestId,
        now: Instant,
    ) -> NativeReplyDisposition {
        if self.connection.as_ref() != Some(connection) {
            return NativeReplyDisposition::IgnoredStale;
        }
        let Some(call) = self.pending.get(&id) else {
            return NativeReplyDisposition::IgnoredStale;
        };
        if call.connection != *connection {
            return NativeReplyDisposition::IgnoredStale;
        }
        let dispatched = call.dispatched;
        let expired = call.deadline <= now;
        if expired {
            if self.pending.remove(&id).is_none() {
                return NativeReplyDisposition::IgnoredStale;
            }
            return NativeReplyDisposition::Expired(if dispatched {
                NativeCallOutcome::Uncertain
            } else {
                NativeCallOutcome::TimedOutBeforeDispatch
            });
        }
        if !dispatched {
            return NativeReplyDisposition::IgnoredBeforeDispatch;
        }
        self.pending.remove(&id);
        NativeReplyDisposition::Accepted
    }

    fn expire(&mut self, now: Instant) -> Vec<NativeCallRetirement> {
        let expired = self
            .pending
            .iter()
            .filter_map(|(id, call)| (call.deadline <= now).then_some(*id))
            .collect::<Vec<_>>();
        expired
            .into_iter()
            .filter_map(|id| {
                self.pending.remove(&id).map(|call| NativeCallRetirement {
                    id,
                    outcome: if call.dispatched {
                        NativeCallOutcome::Uncertain
                    } else {
                        NativeCallOutcome::TimedOutBeforeDispatch
                    },
                })
            })
            .collect()
    }

    fn require_current(&self, connection: &NativeConnection) -> Result<(), NativeCallError> {
        if self.connection.as_ref() == Some(connection) {
            Ok(())
        } else {
            Err(NativeCallError::StaleConnection)
        }
    }

    fn retire_pending(&mut self) -> Vec<NativeCallRetirement> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(id, call)| NativeCallRetirement {
                id,
                outcome: if call.dispatched {
                    NativeCallOutcome::Uncertain
                } else {
                    NativeCallOutcome::DisconnectedBeforeDispatch
                },
            })
            .collect()
    }
}

type AppHandle = codeterm_host_runtime::app_runtime::AppHandle;

pub struct TauriUiEvents(pub AppHandle);

impl UiEvents for TauriUiEvents {
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, &payload);
        if let Some(events) = self.0.try_state::<codeterm_events::EventBroadcasterState>() {
            events.publish_generic(
                event,
                codeterm_events::web_safe_event_payload(event, payload),
            );
        }
    }

    fn emit_native(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
    }

    fn emit_to(&self, target: &str, event: &str, payload: Value) {
        let _ = self.0.emit_to(target, event, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    fn connection(server_epoch: &str, generation: u64) -> NativeConnection {
        NativeConnection {
            server_epoch: server_epoch.to_owned(),
            generation,
        }
    }

    #[test]
    fn native_calls_are_unique_and_pending_work_is_bounded() {
        let now = Instant::now();
        let connection = connection("epoch-a", 1);
        let mut calls = NativeCallRegistry::default();
        calls.attach(connection.clone());

        let ids = (0..MAX_PENDING_NATIVE_CALLS)
            .map(|_| {
                calls
                    .begin(&connection, now, Duration::from_secs(30))
                    .unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(calls.pending.len(), MAX_PENDING_NATIVE_CALLS);
        assert_eq!(
            calls.begin(&connection, now, Duration::from_secs(30)),
            Err(NativeCallError::PendingLimit)
        );
        assert_eq!(
            calls.cancel(&connection, ids[0]),
            Ok(NativeCallOutcome::CancelledBeforeDispatch)
        );
        let next = calls
            .begin(&connection, now, Duration::from_secs(30))
            .unwrap();
        assert!(ids.iter().all(|id| *id != next));
        assert_eq!(calls.pending.len(), MAX_PENDING_NATIVE_CALLS);
    }

    #[test]
    fn reconnect_retires_old_generation_and_discards_late_replies() {
        let now = Instant::now();
        let old = connection("epoch-a", 4);
        let current = connection("epoch-a", 5);
        let mut calls = NativeCallRegistry::default();
        calls.attach(old.clone());
        let id = calls.begin(&old, now, Duration::from_secs(30)).unwrap();
        calls.mark_dispatched(&old, id, now).unwrap();

        let retired = calls.attach(current.clone());

        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].id, id);
        assert_eq!(retired[0].outcome, NativeCallOutcome::Uncertain);
        assert_eq!(
            calls.accept_reply(&old, id, now),
            NativeReplyDisposition::IgnoredStale
        );
        assert_eq!(
            calls.begin(&current, now, Duration::from_secs(30)),
            Ok(NativeRequestId(id.0 + 1))
        );
    }

    #[test]
    fn disconnect_retires_pending_calls_without_replay() {
        let now = Instant::now();
        let connection = connection("epoch-a", 2);
        let mut calls = NativeCallRegistry::default();
        calls.attach(connection.clone());
        let queued = calls
            .begin(&connection, now, Duration::from_secs(30))
            .unwrap();
        let dispatched = calls
            .begin(&connection, now, Duration::from_secs(30))
            .unwrap();
        calls.mark_dispatched(&connection, dispatched, now).unwrap();

        let retired = calls.detach(&connection);

        assert_eq!(retired.len(), 2);
        assert!(retired.iter().any(|call| {
            call.id == queued && call.outcome == NativeCallOutcome::DisconnectedBeforeDispatch
        }));
        assert!(retired
            .iter()
            .any(|call| { call.id == dispatched && call.outcome == NativeCallOutcome::Uncertain }));
        assert_eq!(
            calls.begin(&connection, now, Duration::from_secs(30)),
            Err(NativeCallError::NotAttached)
        );
        assert_eq!(
            calls.accept_reply(&connection, dispatched, now),
            NativeReplyDisposition::IgnoredStale
        );
    }

    #[test]
    fn server_epoch_fences_replies_even_when_generation_matches() {
        let now = Instant::now();
        let old = connection("epoch-a", 1);
        let current = connection("epoch-b", 1);
        let mut calls = NativeCallRegistry::default();
        calls.attach(old.clone());
        let id = calls.begin(&old, now, Duration::from_secs(30)).unwrap();
        calls.mark_dispatched(&old, id, now).unwrap();
        calls.attach(current.clone());

        assert_eq!(
            calls.accept_reply(&old, id, now),
            NativeReplyDisposition::IgnoredStale
        );
        assert_eq!(
            calls.begin(&current, now, Duration::from_secs(30)),
            Ok(NativeRequestId(id.0 + 1))
        );
    }

    #[test]
    fn cancellation_and_deadline_keep_execution_uncertainty() {
        let now = Instant::now();
        let connection = connection("epoch-a", 1);
        let mut calls = NativeCallRegistry::default();
        calls.attach(connection.clone());
        let cancelled = calls
            .begin(&connection, now, Duration::from_secs(1))
            .unwrap();
        let timed_out = calls
            .begin(&connection, now, Duration::from_secs(1))
            .unwrap();
        let cancelled_after_dispatch = calls
            .begin(&connection, now, Duration::from_secs(1))
            .unwrap();
        let dispatched = calls
            .begin(&connection, now, Duration::from_secs(1))
            .unwrap();
        calls
            .mark_dispatched(&connection, cancelled_after_dispatch, now)
            .unwrap();
        calls.mark_dispatched(&connection, dispatched, now).unwrap();

        assert_eq!(
            calls.cancel(&connection, cancelled),
            Ok(NativeCallOutcome::CancelledBeforeDispatch)
        );
        assert_eq!(
            calls.cancel(&connection, cancelled_after_dispatch),
            Ok(NativeCallOutcome::Uncertain)
        );
        assert_eq!(
            calls.accept_reply(&connection, timed_out, now),
            NativeReplyDisposition::IgnoredBeforeDispatch
        );
        let retired = calls.expire(now + Duration::from_secs(1));
        assert_eq!(retired.len(), 2);
        assert!(retired.iter().any(|call| {
            call.id == timed_out && call.outcome == NativeCallOutcome::TimedOutBeforeDispatch
        }));
        assert!(retired
            .iter()
            .any(|call| { call.id == dispatched && call.outcome == NativeCallOutcome::Uncertain }));
    }

    #[test]
    fn late_dispatched_reply_is_expired_as_uncertain() {
        let now = Instant::now();
        let connection = connection("epoch-a", 1);
        let mut calls = NativeCallRegistry::default();
        calls.attach(connection.clone());
        let id = calls
            .begin(&connection, now, Duration::from_secs(1))
            .unwrap();
        calls.mark_dispatched(&connection, id, now).unwrap();

        assert_eq!(
            calls.accept_reply(&connection, id, now + Duration::from_secs(1)),
            NativeReplyDisposition::Expired(NativeCallOutcome::Uncertain)
        );
        assert!(calls.pending.is_empty());
    }

    #[test]
    fn stale_disconnect_cannot_retire_the_current_connection() {
        let now = Instant::now();
        let old = connection("epoch-a", 1);
        let current = connection("epoch-b", 1);
        let mut calls = NativeCallRegistry::default();
        calls.attach(old.clone());
        calls.attach(current.clone());

        assert!(calls.detach(&old).is_empty());
        assert_eq!(
            calls.begin(&current, now, Duration::from_secs(30)),
            Ok(NativeRequestId(1))
        );
    }

    #[test]
    fn ui_events_redact_web_payloads_and_preserve_desktop_payloads() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let events = std::sync::Arc::new(codeterm_events::EventBroadcaster::new());
        let mut web = events.subscribe();
        assert!(app.manage(events));
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen("config-changed", move |event| {
            sender.send(event.payload().to_owned()).unwrap();
        });
        let payload = serde_json::json!({ "api": { "token": "secret", "port": 1234 } });
        TauriUiEvents(app.handle().clone()).emit("config-changed", payload.clone());
        let desktop: serde_json::Value =
            serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
        assert_eq!(desktop, payload);
        match web.try_recv().unwrap().event {
            codeterm_events::SystemEvent::Generic { event_type, data } => {
                assert_eq!(event_type, "config-changed");
                assert_eq!(
                    data,
                    serde_json::json!({ "api": { "token": null, "port": 1234 } })
                );
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn ui_events_reach_desktop_and_web_subscribers() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let events = std::sync::Arc::new(codeterm_events::EventBroadcaster::new());
        let mut web = events.subscribe();
        assert!(app.manage(events));
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen("host-probe", move |event| {
            sender.send(event.payload().to_owned()).unwrap();
        });
        let ui = TauriUiEvents(app.handle().clone());
        let payload = serde_json::json!({ "probe": [1, 2] });
        ui.emit("host-probe", payload.clone());
        let desktop: Value = serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
        assert_eq!(desktop, payload);
        match web.try_recv().unwrap().event {
            codeterm_events::SystemEvent::Generic { event_type, data } => {
                assert_eq!(event_type, "host-probe");
                assert_eq!(data, payload);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        let (native_sender, native_receiver) = std::sync::mpsc::channel();
        app.listen("native-probe", move |event| {
            native_sender.send(event.payload().to_owned()).unwrap();
        });
        ui.emit_native("native-probe", serde_json::json!({ "native": true }));
        let native: Value = serde_json::from_str(&native_receiver.try_recv().unwrap()).unwrap();
        assert_eq!(native, serde_json::json!({ "native": true }));
        assert!(web.try_recv().is_err());
        ui.emit_to("main", "targeted-probe", Value::Null);
        assert!(web.try_recv().is_err());
    }
}

pub struct TauriShell(pub AppHandle);

fn failed(error: impl std::fmt::Display) -> ShellError {
    ShellError::Failed(error.to_string())
}

impl TauriShell {
    fn on_main<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ShellError> + Send + 'static,
    ) -> Result<T, ShellError> {
        let app = self.0.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let phase = std::sync::Arc::new(AtomicU8::new(MAIN_THREAD_QUEUED));
        let scheduled_phase = phase.clone();
        let permit = MainThreadQueuePermit::acquire()?;
        self.0
            .run_on_main_thread(move || {
                let _permit = permit;
                if scheduled_phase
                    .compare_exchange(
                        MAIN_THREAD_QUEUED,
                        MAIN_THREAD_RUNNING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_err()
                {
                    return;
                }
                let _ = sender.send(action(app));
                scheduled_phase.store(MAIN_THREAD_COMPLETED, Ordering::Release);
            })
            .map_err(failed)?;
        match receiver.recv_timeout(MAIN_THREAD_ACK_DEADLINE) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if phase
                    .compare_exchange(
                        MAIN_THREAD_QUEUED,
                        MAIN_THREAD_CANCELLED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    Err(ShellError::Failed(
                        "Main-thread action cancelled before execution at its acknowledgement deadline".into(),
                    ))
                } else {
                    match receiver.try_recv() {
                        Ok(result) => result,
                        Err(_) => Err(ShellError::Failed(
                            "Main-thread action acknowledgement deadline elapsed after execution began; outcome is uncertain".into(),
                        )),
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if phase
                    .compare_exchange(
                        MAIN_THREAD_QUEUED,
                        MAIN_THREAD_CANCELLED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    Err(ShellError::Failed(
                        "Main-thread action cancelled before execution because its acknowledgement channel closed".into(),
                    ))
                } else {
                    Err(ShellError::Failed(
                        "Main-thread action acknowledgement channel closed after execution began; outcome is uncertain".into(),
                    ))
                }
            }
        }
    }
}

impl Shell for TauriShell {
    fn resource_dir(&self) -> Result<PathBuf, ShellError> {
        self.0.path().resource_dir().map_err(failed)
    }

    fn focus_window(&self, label: &str) -> Result<(), ShellError> {
        let label = label.to_owned();
        self.on_main(move |app| {
            if label == "main" {
                return crate::startup::desktop_lifecycle::show_or_create(&app).map_err(failed);
            }
            if let Some(window) = app.get_webview_window(&label) {
                window.show().map_err(failed)?;
                window.unminimize().map_err(failed)?;
                window.set_focus().map_err(failed)?;
            }
            Ok(())
        })
    }

    fn window_exists(&self, label: &str) -> bool {
        let label = label.to_owned();
        self.on_main(move |app| Ok(app.get_webview_window(&label).is_some()))
            .unwrap_or(false)
    }

    fn os_notify(&self, title: &str, body: Option<&str>) -> Result<(), ShellError> {
        crate::os_notify::_send_os_notification(&self.0, title, body);
        Ok(())
    }

    fn toast_permission(&self, enabling: Option<bool>) -> String {
        match enabling {
            Some(enabling) => crate::os_notify::establish_toast_permission(&self.0, enabling),
            None => crate::os_notify::toast_permission(&self.0),
        }
        .to_string()
    }

    fn open_webview(&self, request: WebviewRequest) -> Result<(), ShellError> {
        let url = request.url.parse().map_err(failed)?;
        self.on_main(move |app| {
            tauri::WebviewWindowBuilder::new(&app, request.label, tauri::WebviewUrl::External(url))
                .title(request.title)
                .inner_size(request.width, request.height)
                .build()
                .map_err(failed)?;
            Ok(())
        })
    }

    fn current_exe(&self) -> Result<PathBuf, ShellError> {
        tauri::utils::platform::current_exe().map_err(failed)
    }

    fn config_dir(&self) -> Result<PathBuf, ShellError> {
        self.0.path().config_dir().map_err(failed)
    }
}
