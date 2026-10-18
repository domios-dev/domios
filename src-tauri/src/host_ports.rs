#[cfg(test)]
use std::time::Instant;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use codeterm_service_core::ports::host::{Shell, ShellError, UiEvents, WebviewRequest};
use codeterm_types::{
    native_host::{NativeHostRejection, NativeHostUncertainty},
    server_bootstrap::{ServerIdentity, ServerReady},
};
use reqwest::redirect::Policy;
use tauri::{Emitter, Manager};

pub(crate) mod native_client;
pub(crate) mod webview_diagnostics;
pub(crate) use webview_diagnostics::{ClientViewGeneration, CLIENT_VIEW_QUERY};

#[cfg(test)]
const MAX_PENDING_NATIVE_CALLS: usize = 64;

const MAX_QUEUED_MAIN_THREAD_CALLS: usize = 64;
const MAIN_THREAD_ACK_DEADLINE: Duration = Duration::from_secs(10);
const MAIN_THREAD_QUEUED: u8 = 0;
const MAIN_THREAD_RUNNING: u8 = 1;
const MAIN_THREAD_CANCELLED: u8 = 2;
const MAIN_THREAD_COMPLETED: u8 = 3;
static QUEUED_MAIN_THREAD_CALLS: AtomicUsize = AtomicUsize::new(0);

const SERVER_API_REQUEST_DEADLINE: Duration = Duration::from_secs(10);
const REQUEST_REPLAY_WINDOW: u64 = 256;

#[derive(Default)]
pub(crate) struct RequestReplayWindow {
    highest: u64,
    seen: HashSet<u64>,
}

impl RequestReplayWindow {
    pub(crate) fn accept(&mut self, request_id: u64) -> bool {
        let floor = self.highest.saturating_sub(REQUEST_REPLAY_WINDOW);
        if request_id == 0 || request_id <= floor || self.seen.contains(&request_id) {
            return false;
        }
        self.highest = self.highest.max(request_id);
        let floor = self.highest.saturating_sub(REQUEST_REPLAY_WINDOW);
        self.seen.retain(|seen| *seen > floor);
        self.seen.insert(request_id)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ClientRootRenderState {
    generation: Option<ClientViewGeneration>,
    origin: Option<String>,
    acknowledged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientRootRenderWait {
    Ready,
    Replaced,
    Deadline,
}

#[derive(Clone)]
pub(crate) struct ClientRootRenderLatch(tokio::sync::watch::Sender<ClientRootRenderState>);

impl Default for ClientRootRenderLatch {
    fn default() -> Self {
        let (sender, _) = tokio::sync::watch::channel(ClientRootRenderState::default());
        Self(sender)
    }
}

impl ClientRootRenderLatch {
    pub(crate) fn arm(&self, generation: ClientViewGeneration, origin: String) {
        self.0.send_replace(ClientRootRenderState {
            generation: Some(generation),
            origin: Some(origin),
            acknowledged: false,
        });
    }

    pub(crate) fn acknowledge(&self, generation: ClientViewGeneration, origin: &str) -> bool {
        let mut accepted = false;
        self.0.send_if_modified(|state| {
            if state.generation != Some(generation) || state.origin.as_deref() != Some(origin) {
                return false;
            }
            accepted = true;
            if state.acknowledged {
                return false;
            }
            state.acknowledged = true;
            true
        });
        accepted
    }

    pub(crate) fn is_armed(&self, generation: ClientViewGeneration, origin: &str) -> bool {
        let state = self.0.borrow();
        state.generation == Some(generation) && state.origin.as_deref() == Some(origin)
    }

    pub(crate) fn retire(&self) {
        self.0.send_replace(ClientRootRenderState::default());
    }

    pub(crate) async fn wait_until(
        &self,
        generation: ClientViewGeneration,
        deadline: std::time::Instant,
    ) -> ClientRootRenderWait {
        let mut state = self.0.subscribe();
        let deadline = tokio::time::Instant::from_std(deadline);
        loop {
            let current = state.borrow_and_update().clone();
            if current.generation != Some(generation) {
                return ClientRootRenderWait::Replaced;
            }
            if current.acknowledged {
                return ClientRootRenderWait::Ready;
            }
            match tokio::time::timeout_at(deadline, state.changed()).await {
                Err(_) => return ClientRootRenderWait::Deadline,
                Ok(Err(_)) => return ClientRootRenderWait::Replaced,
                Ok(Ok(())) => {}
            }
        }
    }
}

#[tauri::command]
pub(crate) async fn frontend_view_root_ready(
    window: codeterm_host_runtime::app_runtime::WebviewWindow,
    view_generation: u64,
    navigation_generation: u64,
) -> Result<bool, String> {
    if window.label() != "main" || view_generation == 0 || navigation_generation == 0 {
        return Ok(false);
    }
    let generation = ClientViewGeneration {
        view: view_generation,
        navigation: navigation_generation,
    };
    let url = window.url().map_err(|error| error.to_string())?;
    if !crate::startup::frontend_shell::active_view_matches(&window.app_handle(), generation.view)
        || webview_diagnostics::root_generation_from_url(&url) != Some(generation)
    {
        return Ok(false);
    }
    let app = window.app_handle();
    let latch = app
        .try_state::<ClientRootRenderLatch>()
        .ok_or_else(|| "Native root-render observer is unavailable".to_string())?;
    let accepted = latch.acknowledge(generation, &url.origin().ascii_serialization());
    if accepted {
        if let Some(slot) = app.try_state::<native_client::NativeHostClientSlot>() {
            slot.report_root_rendered(generation, url).await;
        }
    }
    Ok(accepted)
}

#[derive(Clone, Default)]
pub(crate) struct ServerApiAttachmentSlot(Arc<parking_lot::RwLock<Option<ServerApiAttachment>>>);

#[derive(Clone)]
pub(crate) struct ServerApiAttachment {
    ready: ServerReady,
    token: String,
    client: reqwest::Client,
}

impl ServerApiAttachmentSlot {
    pub(crate) fn install(&self, ready: ServerReady, token: String) -> Result<(), String> {
        if !ready.api_address.ip().is_loopback() || token.trim().is_empty() {
            return Err("Authenticated server attachment is invalid".into());
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(SERVER_API_REQUEST_DEADLINE)
            .build()
            .map_err(|error| error.to_string())?;
        *self.0.write() = Some(ServerApiAttachment {
            ready,
            token,
            client,
        });
        Ok(())
    }

    pub(crate) fn current(&self) -> Option<ServerApiAttachment> {
        self.0.read().clone()
    }
}

impl ServerApiAttachment {
    pub(crate) fn ready(&self) -> &ServerReady {
        &self.ready
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn identity(&self) -> &ServerIdentity {
        &self.ready.identity
    }

    pub(crate) fn epoch(&self) -> &str {
        &self.ready.epoch
    }

    pub(crate) fn api_origin(&self) -> String {
        format!("http://{}", self.ready.api_address)
    }

    pub(crate) async fn refresh_plan(&self) -> Result<(), String> {
        self.post("/api/account/plan/refresh").await
    }

    pub(crate) async fn save_session(&self) -> Result<(), String> {
        self.post("/api/session/save").await
    }

    pub(crate) async fn post_account_callback(&self, url: &str) -> Result<(), String> {
        if url.len() > 4 * 1024 {
            return Err("Account callback exceeds the client request limit".into());
        }
        let response = self
            .client
            .post(format!("{}/api/account/callback", self.api_origin()))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "url": url }))
            .send()
            .await
            .map_err(|_| "Account callback delivery outcome is uncertain".to_string())?;
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err("Server did not accept the account callback".into())
        }
    }

    async fn post(&self, route: &str) -> Result<(), String> {
        self.client
            .post(format!("{}{}", self.api_origin(), route))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({}))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

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

    #[tokio::test]
    async fn process_split_contract_root_render_ack_is_origin_and_generation_bound() {
        let latch = ClientRootRenderLatch::default();
        let first = ClientViewGeneration {
            view: 4,
            navigation: 9,
        };
        latch.arm(first, "http://127.0.0.1:7686".into());

        assert!(!latch.acknowledge(first, "http://127.0.0.1:7685"));
        assert!(!latch.acknowledge(
            ClientViewGeneration {
                navigation: 8,
                ..first
            },
            "http://127.0.0.1:7686"
        ));
        assert!(latch.acknowledge(first, "http://127.0.0.1:7686"));
        assert!(latch.acknowledge(first, "http://127.0.0.1:7686"));
        assert!(latch.is_armed(first, "http://127.0.0.1:7686"));
        assert_eq!(
            latch
                .wait_until(first, std::time::Instant::now() + Duration::from_secs(1))
                .await,
            ClientRootRenderWait::Ready
        );

        let replacement = ClientViewGeneration {
            navigation: 10,
            ..first
        };
        latch.arm(replacement, "http://127.0.0.1:7686".into());
        assert!(!latch.acknowledge(first, "http://127.0.0.1:7686"));
        latch.retire();
        assert!(!latch.acknowledge(replacement, "http://127.0.0.1:7686"));
        assert_eq!(
            latch
                .wait_until(
                    replacement,
                    std::time::Instant::now() + Duration::from_secs(1)
                )
                .await,
            ClientRootRenderWait::Replaced
        );
    }

    fn connection(server_epoch: &str, generation: u64) -> NativeConnection {
        NativeConnection {
            server_epoch: server_epoch.to_owned(),
            generation,
        }
    }

    #[test]
    fn process_split_contract_native_calls_are_unique_and_pending_work_is_bounded() {
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
    fn process_split_contract_native_request_ids_reject_replay_and_allow_bounded_reordering() {
        let mut ids = RequestReplayWindow::default();
        assert!(ids.accept(1));
        assert!(ids.accept(3));
        assert!(ids.accept(2));
        assert!(!ids.accept(2));
        for request_id in 4..=260 {
            assert!(ids.accept(request_id));
        }
        assert!(!ids.accept(3));
    }

    #[test]
    fn process_split_contract_reconnect_retires_old_generation_and_discards_late_replies() {
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
    fn process_split_contract_disconnect_retires_pending_calls_without_replay() {
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
    fn process_split_contract_server_epoch_fences_replies_even_when_generation_matches() {
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
    fn process_split_contract_cancellation_and_deadline_keep_execution_uncertainty() {
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
    fn process_split_contract_late_dispatched_reply_is_expired_as_uncertain() {
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
    fn process_split_contract_stale_disconnect_cannot_retire_the_current_connection() {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeShellFailure {
    Refused(NativeHostRejection),
    Uncertain(NativeHostUncertainty),
}

enum MainThreadCallError {
    QueueFull,
    Scheduling(ShellError),
    Action(ShellError),
    DeadlineBeforeDispatch,
    UncertainAfterDispatch,
}

fn failed(error: impl std::fmt::Display) -> ShellError {
    ShellError::Failed(error.to_string())
}

impl TauriShell {
    fn on_main_call<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ShellError> + Send + 'static,
    ) -> Result<T, MainThreadCallError> {
        let app = self.0.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let phase = std::sync::Arc::new(AtomicU8::new(MAIN_THREAD_QUEUED));
        let scheduled_phase = phase.clone();
        let permit =
            MainThreadQueuePermit::acquire().map_err(|_| MainThreadCallError::QueueFull)?;
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
            .map_err(|error| MainThreadCallError::Scheduling(failed(error)))?;
        match receiver.recv_timeout(MAIN_THREAD_ACK_DEADLINE) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(MainThreadCallError::Action(error)),
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
                    Err(MainThreadCallError::DeadlineBeforeDispatch)
                } else {
                    match receiver.try_recv() {
                        Ok(Ok(result)) => Ok(result),
                        Ok(Err(error)) => Err(MainThreadCallError::Action(error)),
                        Err(_) => Err(MainThreadCallError::UncertainAfterDispatch),
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
                    Err(MainThreadCallError::DeadlineBeforeDispatch)
                } else {
                    Err(MainThreadCallError::UncertainAfterDispatch)
                }
            }
        }
    }

    fn on_main<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ShellError> + Send + 'static,
    ) -> Result<T, ShellError> {
        self.on_main_call(action).map_err(|error| match error {
            MainThreadCallError::Scheduling(error) | MainThreadCallError::Action(error) => error,
            MainThreadCallError::QueueFull => {
                ShellError::Failed("Native main-thread queue is full".into())
            }
            MainThreadCallError::DeadlineBeforeDispatch => ShellError::Failed(
                "Main-thread action was cancelled before execution at its deadline".into(),
            ),
            MainThreadCallError::UncertainAfterDispatch => ShellError::Failed(
                "Main-thread action acknowledgement elapsed after execution began".into(),
            ),
        })
    }

    fn on_main_native<T: Send + 'static>(
        &self,
        action: impl FnOnce(AppHandle) -> Result<T, ShellError> + Send + 'static,
    ) -> Result<T, NativeShellFailure> {
        self.on_main_call(action).map_err(|error| match error {
            MainThreadCallError::QueueFull => {
                NativeShellFailure::Refused(NativeHostRejection::QueueFull)
            }
            MainThreadCallError::Scheduling(_) => {
                NativeShellFailure::Refused(NativeHostRejection::HostRejected)
            }
            MainThreadCallError::DeadlineBeforeDispatch => {
                NativeShellFailure::Refused(NativeHostRejection::DeadlineBeforeDispatch)
            }
            MainThreadCallError::Action(_) | MainThreadCallError::UncertainAfterDispatch => {
                NativeShellFailure::Uncertain(NativeHostUncertainty::DisconnectedAfterDispatch)
            }
        })
    }

    pub(crate) fn focus_window_native(&self, label: String) -> Result<(), NativeShellFailure> {
        self.on_main_native(move |app| {
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

    pub(crate) fn window_exists_native(&self, label: String) -> Result<bool, NativeShellFailure> {
        self.on_main_native(move |app| Ok(app.get_webview_window(&label).is_some()))
    }

    pub(crate) fn open_webview_native(
        &self,
        label: String,
        url: url::Url,
        title: String,
        width: f64,
        height: f64,
    ) -> Result<(), NativeShellFailure> {
        self.on_main_native(move |app| {
            tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::External(url))
                .title(title)
                .inner_size(width, height)
                .build()
                .map_err(failed)?;
            Ok(())
        })
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
