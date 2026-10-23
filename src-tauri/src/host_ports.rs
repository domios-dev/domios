#[cfg(test)]
use std::time::Instant;
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use codeterm_types::server_bootstrap::{ServerIdentity, ServerReady};
use reqwest::redirect::Policy;
use tauri::Manager;
pub(crate) mod native_client;
pub(crate) mod native_shell;
pub(crate) mod update_pair;
pub(crate) use native_shell::{NativeShellFailure, TauriShell};
pub(crate) mod webview_diagnostics;
pub(crate) use webview_diagnostics::ClientViewGeneration;

#[cfg(test)]
const MAX_PENDING_NATIVE_CALLS: usize = 64;

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

    pub(crate) fn acknowledged_generation(&self, origin: &str) -> Option<ClientViewGeneration> {
        let state = self.0.borrow();
        if state.acknowledged && state.origin.as_deref() == Some(origin) {
            state.generation
        } else {
            None
        }
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
    window: codeterm_client_shell::app_runtime::WebviewWindow,
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

#[tauri::command]
pub(crate) fn native_webview_diagnostics_snapshot(
    window: codeterm_client_shell::app_runtime::WebviewWindow,
) -> Option<codeterm_types::native_host::NativeWebviewDiagnosticsSnapshot> {
    if window.label() != "main" {
        return None;
    }
    let app = window.app_handle();
    let url = window.url().ok()?;
    let generation = app
        .try_state::<ClientRootRenderLatch>()?
        .acknowledged_generation(&url.origin().ascii_serialization())?;
    if webview_diagnostics::root_generation_from_url(&url)
        .is_some_and(|supplied| supplied != generation)
        || !crate::startup::frontend_shell::active_view_matches(&app, generation.view)
    {
        return None;
    }
    let slot = app.try_state::<native_client::NativeHostClientSlot>()?;
    slot.webview_diagnostics_snapshot(generation)
}

#[derive(Clone, Default)]
pub(crate) struct ServerApiAttachmentSlot(Arc<parking_lot::RwLock<Option<ServerApiAttachment>>>);

#[derive(Clone)]
pub(crate) struct ServerApiAttachment {
    ready: ServerReady,
    api_address: SocketAddr,
    token: String,
    client: reqwest::Client,
}

impl ServerApiAttachmentSlot {
    pub(crate) fn install(&self, ready: ServerReady, token: String) -> Result<(), String> {
        let Some(api_address) = ready
            .loopback_api_address()
            .filter(|_| !token.trim().is_empty())
        else {
            return Err("Authenticated server attachment is invalid".into());
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(SERVER_API_REQUEST_DEADLINE)
            .build()
            .map_err(|error| error.to_string())?;
        *self.0.write() = Some(ServerApiAttachment {
            ready,
            api_address,
            token,
            client,
        });
        Ok(())
    }

    pub(crate) fn current(&self) -> Option<ServerApiAttachment> {
        self.0.read().clone()
    }

    pub(crate) fn clear_if_current(&self, expected: &ServerReady) -> bool {
        let mut current = self.0.write();
        if current
            .as_ref()
            .is_some_and(|active| active.ready() == expected)
        {
            current.take();
            true
        } else {
            false
        }
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
        format!("http://{}", self.api_address)
    }

    pub(crate) async fn refresh_plan(&self) -> Result<(), String> {
        self.post("/api/account/plan/refresh").await
    }

    pub(crate) async fn refresh_plan_on_focus(&self) -> Result<(), String> {
        self.post("/api/account/plan/refresh?focus=true").await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_launch_requires_typed_absent_endpoint() {
        use native_client::HelperFailure;

        assert!(HelperFailure::Exit {
            code: Some(codeterm_types::server_bootstrap::BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE),
            request_forwarded: true,
            detail: None,
        }
        .allows_initial_server_launch());
        assert!(!HelperFailure::Exit {
            code: Some(codeterm_types::server_bootstrap::BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE),
            request_forwarded: false,
            detail: None,
        }
        .allows_initial_server_launch());
        assert!(!HelperFailure::Exit {
            code: Some(1),
            request_forwarded: true,
            detail: None,
        }
        .allows_initial_server_launch());
        assert!(!HelperFailure::Exit {
            code: Some(3),
            request_forwarded: true,
            detail: None,
        }
        .allows_initial_server_launch());
        assert!(!HelperFailure::PreDispatch(
            native_client::ExactServerShutdownRefusal::StaleIncarnation,
        )
        .allows_initial_server_launch());
        assert!(!HelperFailure::Deadline {
            request_forwarded: true,
        }
        .allows_initial_server_launch());
        assert!(!HelperFailure::Io {
            message: "unexpected helper transport error".into(),
            request_forwarded: true,
        }
        .allows_initial_server_launch());
    }

    #[test]
    fn stated_helper_refusal_reaches_the_startup_failure_text() {
        use native_client::{ClientStartupFailure, HelperFailure};

        let failure = ClientStartupFailure::launch(&HelperFailure::Exit {
            code: Some(1),
            request_forwarded: true,
            detail: Some("opaque-cause-17".into()),
        });
        assert!(failure.stated_refusal());
        assert!(failure.message().ends_with(": opaque-cause-17"));
        assert!(!ClientStartupFailure::launch(&HelperFailure::Exit {
            code: Some(1),
            request_forwarded: true,
            detail: None,
        })
        .stated_refusal());
        assert!(!ClientStartupFailure::from("opaque-cause-17").stated_refusal());
    }

    #[test]
    fn diagnostics_generation_survives_cosmetic_query_cleanup_but_not_replacement() {
        let latch = ClientRootRenderLatch::default();
        let generation = ClientViewGeneration {
            view: 2,
            navigation: 3,
        };
        let origin = "http://127.0.0.1:1420";
        latch.arm(generation, origin.into());
        assert_eq!(latch.acknowledged_generation(origin), None);
        assert!(latch.acknowledge(generation, origin));
        assert_eq!(latch.acknowledged_generation(origin), Some(generation));
        assert_eq!(latch.acknowledged_generation("http://127.0.0.1:7685"), None);
        latch.arm(
            ClientViewGeneration {
                navigation: 4,
                ..generation
            },
            origin.into(),
        );
        assert_eq!(latch.acknowledged_generation(origin), None);
        latch.retire();
        assert_eq!(latch.acknowledged_generation(origin), None);
    }

    #[tokio::test]
    async fn helper_frame_accepts_fragmented_single_line_and_eof() {
        let expected = b"{\"ready\":true}\n";
        let mut reader =
            tokio::io::BufReader::with_capacity(2, std::io::Cursor::new(expected.to_vec()));

        assert_eq!(
            native_client::read_single_bounded_line(&mut reader, expected.len())
                .await
                .unwrap(),
            expected
        );
    }

    #[tokio::test]
    async fn helper_frame_rejects_oversized_unterminated_bytes_before_growth() {
        let mut reader =
            tokio::io::BufReader::with_capacity(3, std::io::Cursor::new(vec![b'x'; 33]));

        assert_eq!(
            native_client::read_single_bounded_line(&mut reader, 32)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn helper_frame_rejects_missing_newline() {
        let mut reader =
            tokio::io::BufReader::with_capacity(2, std::io::Cursor::new(b"{}".to_vec()));

        assert_eq!(
            native_client::read_single_bounded_line(&mut reader, 32)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn helper_frame_rejects_trailing_bytes_and_second_frames() {
        for bytes in [b"{}\nx".to_vec(), b"{}\n{}\n".to_vec()] {
            let mut reader = tokio::io::BufReader::with_capacity(2, std::io::Cursor::new(bytes));

            assert_eq!(
                native_client::read_single_bounded_line(&mut reader, 32)
                    .await
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::InvalidData
            );
        }
    }

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
}
