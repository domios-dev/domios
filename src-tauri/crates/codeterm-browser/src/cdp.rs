use super::cdp_wire::{
    decode_document_incoming, decode_incoming, discard_value, encode_request, response_id,
    CdpEvent, EventFilter, Incoming, ProtocolError, SessionFilter,
};
use super::launch::validate_loopback_endpoint;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

/// Full-page screenshots and AX trees are large; the cap is explicit so overflow is a logged error.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
pub const CONNECT_DEADLINE: Duration = Duration::from_secs(10);
pub const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const SUBSCRIBER_CAPACITY: usize = 256;
const OUTGOING_CAPACITY: usize = 256;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub fn websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_SIZE))
        .max_frame_size(Some(MAX_MESSAGE_SIZE))
}

#[derive(Clone, Debug, PartialEq)]
pub enum CdpError {
    Closed(String),
    Protocol(ProtocolError),
    Deadline(String),
    Malformed(String),
    Cancelled(String),
}

impl std::fmt::Display for CdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed(reason) => write!(f, "CDP connection closed: {reason}"),
            Self::Protocol(error) => write!(f, "CDP error {}: {}", error.code, error.message),
            Self::Deadline(method) => write!(f, "CDP {method} got no reply before its deadline"),
            Self::Malformed(detail) => write!(f, "unexpected CDP reply: {detail}"),
            Self::Cancelled(reason) => write!(f, "CDP dispatch cancelled: {reason}"),
        }
    }
}

/// Checked immediately before enqueuing and immediately before writing a guarded command to CDP.
pub type DispatchGate = Arc<dyn Fn() -> bool + Send + Sync>;

type Reply = Result<Value, CdpError>;

struct PendingReply {
    sender: oneshot::Sender<Reply>,
    document: bool,
}

struct OutgoingBatch {
    messages: Vec<Message>,
    ids: Vec<u64>,
    session_id: Option<String>,
    guarded_dialog: bool,
    captured_dialog_epoch: u64,
    gate: Option<DispatchGate>,
}

impl OutgoingBatch {
    fn plain(messages: Vec<Message>) -> Self {
        Self {
            messages,
            ids: Vec::new(),
            session_id: None,
            guarded_dialog: false,
            captured_dialog_epoch: 0,
            gate: None,
        }
    }
}

struct Subscriber {
    id: u64,
    filter: EventFilter,
    sender: mpsc::Sender<Arc<CdpEvent>>,
    dropped: Arc<AtomicU64>,
}

#[derive(Default)]
struct State {
    pending: HashMap<u64, PendingReply>,
    subscribers: Vec<Subscriber>,
    closed: Option<String>,
    wire_dialog_epoch: HashMap<String, u64>,
    wire_dialog_beforeunload: HashMap<String, bool>,
}

struct Inner {
    endpoint: String,
    next_id: AtomicU64,
    next_subscriber: AtomicU64,
    state: Mutex<State>,
    outgoing: mpsc::Sender<OutgoingBatch>,
    closed: watch::Sender<Option<String>>,
}

struct PendingRequests {
    client: Weak<Inner>,
    ids: Vec<u64>,
}

impl Drop for PendingRequests {
    fn drop(&mut self) {
        if let Some(client) = self.client.upgrade() {
            let mut state = client.state.lock();
            for id in &self.ids {
                state.pending.remove(id);
            }
        }
    }
}

impl Inner {
    async fn route(&self, incoming: Incoming) {
        match incoming {
            Incoming::Response { id, result } => {
                let reply = self.state.lock().pending.remove(&id);
                if let Some(reply) = reply {
                    if let Err(Ok(value)) = reply.sender.send(result.map_err(CdpError::Protocol)) {
                        discard_value(value);
                    }
                } else if let Ok(value) = result {
                    discard_value(value);
                }
            }
            Incoming::Event(event) => {
                let event = Arc::new(event);
                self.note_dialog_event(&event);
                self.fan_out(event);
            }
        }
    }

    fn note_dialog_event(&self, event: &CdpEvent) {
        let Some(sid) = event.session_id.as_deref() else {
            return;
        };
        let is_beforeunload = match event.method.as_str() {
            "Page.javascriptDialogOpening" => {
                event.params.get("type").and_then(Value::as_str) == Some("beforeunload")
            }
            "Page.javascriptDialogClosed" => false,
            _ => return,
        };
        let mut state = self.state.lock();
        let epoch = state.wire_dialog_epoch.entry(sid.to_string()).or_insert(0);
        *epoch += 1;
        state
            .wire_dialog_beforeunload
            .insert(sid.to_string(), is_beforeunload);
    }

    fn should_dispatch(&self, batch: &OutgoingBatch) -> bool {
        if batch.gate.as_ref().is_some_and(|gate| !gate()) {
            return false;
        }
        if batch.guarded_dialog {
            if let Some(sid) = batch.session_id.as_deref() {
                let state = self.state.lock();
                if state.wire_dialog_epoch.get(sid).copied().unwrap_or(0)
                    != batch.captured_dialog_epoch
                {
                    return false;
                }
                if state.wire_dialog_beforeunload.get(sid) == Some(&false) {
                    return false;
                }
            }
        }
        true
    }

    fn cancel_pending(&self, ids: &[u64], reason: &str) {
        let replies: Vec<PendingReply> = {
            let mut state = self.state.lock();
            ids.iter()
                .filter_map(|id| state.pending.remove(id))
                .collect()
        };
        for reply in replies {
            let _ = reply
                .sender
                .send(Err(CdpError::Cancelled(reason.to_string())));
        }
    }

    fn fan_out(&self, event: Arc<CdpEvent>) {
        let mut state = self.state.lock();
        state.subscribers.retain_mut(|subscriber| {
            if !subscriber.filter.matches(&event) {
                return true;
            }
            match subscriber.sender.try_send(event.clone()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    let dropped = subscriber.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                    if dropped.is_power_of_two() {
                        log::warn!(
                            "[browser-cdp] subscriber {} is behind; dropped {} events (latest {})",
                            subscriber.id,
                            dropped,
                            event.method
                        );
                    }
                    true
                }
                Err(TrySendError::Closed(_)) => false,
            }
        });
    }

    fn close(&self, reason: &str) {
        let pending = {
            let mut state = self.state.lock();
            if state.closed.is_some() {
                return;
            }
            state.closed = Some(reason.to_string());
            state.subscribers.clear();
            std::mem::take(&mut state.pending)
        };
        for (_, reply) in pending {
            let _ = reply.sender.send(Err(CdpError::Closed(reason.to_string())));
        }
        self.closed.send_replace(Some(reason.to_string()));
    }
}

/// Browser-level CDP connection with flattened target sessions; owned by Rust, never the webview.
#[derive(Clone)]
struct SocketConnection(Arc<Inner>);

impl SocketConnection {
    pub async fn connect(endpoint: &str) -> Result<Self, String> {
        validate_loopback_endpoint(endpoint, &["ws"])?;
        let request = endpoint
            .into_client_request()
            .map_err(|error| format!("invalid CDP endpoint {endpoint}: {error}"))?;
        let (socket, _) = tokio::time::timeout(
            CONNECT_DEADLINE,
            tokio_tungstenite::connect_async_with_config(request, Some(websocket_config()), true),
        )
        .await
        .map_err(|_| format!("CDP connect to {endpoint} hit its deadline"))?
        .map_err(|error| format!("CDP connect to {endpoint} failed: {error}"))?;
        let (writer, reader) = socket.split();
        let (outgoing, messages) = mpsc::channel(OUTGOING_CAPACITY);
        let (closed, _) = watch::channel(None);
        let inner = Arc::new(Inner {
            endpoint: endpoint.to_string(),
            next_id: AtomicU64::new(1),
            next_subscriber: AtomicU64::new(1),
            state: Mutex::new(State::default()),
            outgoing,
            closed,
        });
        tokio::spawn(write_loop(writer, messages, Arc::downgrade(&inner)));
        tokio::spawn(read_loop(reader, Arc::downgrade(&inner)));
        Ok(Self(inner))
    }

    pub fn endpoint(&self) -> &str {
        &self.0.endpoint
    }

    pub fn close_reason(&self) -> Option<String> {
        self.0.state.lock().closed.clone()
    }

    pub fn closed(&self) -> watch::Receiver<Option<String>> {
        self.0.closed.subscribe()
    }

    pub fn close(&self, reason: &str) {
        self.0.close(reason);
    }

    pub async fn send_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Reply {
        self.send_batch_guarded_to(
            session_id,
            vec![(method.to_string(), params)],
            deadline,
            None,
        )
        .await
        .map(|mut replies| replies.remove(0))
    }

    pub async fn send_guarded_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
        can_dispatch: DispatchGate,
    ) -> Reply {
        self.send_batch_guarded_to(
            session_id,
            vec![(method.to_string(), params)],
            deadline,
            Some(can_dispatch),
        )
        .await
        .map(|mut replies| replies.remove(0))
    }

    async fn send_batch_to(
        &self,
        session_id: Option<&str>,
        commands: Vec<(String, Value)>,
        deadline: Duration,
    ) -> Result<Vec<Value>, CdpError> {
        self.send_batch_guarded_to(session_id, commands, deadline, None)
            .await
    }

    async fn send_batch_guarded_to(
        &self,
        session_id: Option<&str>,
        commands: Vec<(String, Value)>,
        deadline: Duration,
        gate: Option<DispatchGate>,
    ) -> Result<Vec<Value>, CdpError> {
        if gate.as_ref().is_some_and(|check| !check()) {
            return Err(CdpError::Cancelled("dispatch authorization revoked".into()));
        }
        let until = Instant::now() + deadline;
        let guarded_dialog = gate.is_some()
            && commands
                .iter()
                .any(|(method, _)| method == "Page.handleJavaScriptDialog");
        let mut ids = Vec::with_capacity(commands.len());
        let mut responses = Vec::with_capacity(commands.len());
        let mut messages = Vec::with_capacity(commands.len());
        let captured_dialog_epoch = {
            let mut state = self.0.state.lock();
            if let Some(reason) = &state.closed {
                return Err(CdpError::Closed(reason.clone()));
            }
            let epoch = session_id
                .and_then(|sid| state.wire_dialog_epoch.get(sid).copied())
                .unwrap_or(0);
            if guarded_dialog
                && session_id.and_then(|sid| state.wire_dialog_beforeunload.get(sid).copied())
                    == Some(false)
            {
                return Err(CdpError::Cancelled("dialog no longer active".into()));
            }
            for (method, params) in commands {
                let id = self.0.next_id.fetch_add(1, Ordering::Relaxed);
                let (reply, response) = oneshot::channel();
                state.pending.insert(
                    id,
                    PendingReply {
                        sender: reply,
                        document: method == "DOM.getDocument",
                    },
                );
                ids.push(id);
                responses.push((method.clone(), response));
                messages.push(Message::text(encode_request(
                    id, &method, params, session_id,
                )));
            }
            epoch
        };
        let batch = OutgoingBatch {
            messages,
            ids: ids.clone(),
            session_id: session_id.map(str::to_string),
            guarded_dialog,
            captured_dialog_epoch,
            gate,
        };
        let _pending = PendingRequests {
            client: Arc::downgrade(&self.0),
            ids,
        };
        async {
            let permit = tokio::time::timeout_at(until, self.0.outgoing.reserve())
                .await
                .map_err(|_| CdpError::Deadline("input batch queue".into()))?
                .map_err(|_| {
                    CdpError::Closed(
                        self.close_reason()
                            .unwrap_or_else(|| "writer stopped".into()),
                    )
                })?;
            if !self.0.should_dispatch(&batch) {
                return Err(CdpError::Cancelled("dispatch authorization revoked".into()));
            }
            permit.send(batch);
            futures_util::future::try_join_all(responses.into_iter().map(
                |(method, response)| async move {
                    tokio::time::timeout_at(until, response)
                        .await
                        .map_err(|_| CdpError::Deadline(method))?
                        .map_err(|_| {
                            CdpError::Closed(
                                self.close_reason()
                                    .unwrap_or_else(|| "reply dropped".into()),
                            )
                        })?
                },
            ))
            .await
        }
        .await
    }

    pub fn subscribe(&self, filter: EventFilter) -> EventStream {
        let id = self.0.next_subscriber.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel(SUBSCRIBER_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut state = self.0.state.lock();
        if state.closed.is_none() {
            state.subscribers.push(Subscriber {
                id,
                filter,
                sender,
                dropped: dropped.clone(),
            });
        }
        EventStream {
            dropped,
            receiver,
            id,
            client: Arc::downgrade(&self.0),
        }
    }
}

#[async_trait::async_trait]
pub trait CdpConnection: Send + Sync {
    fn endpoint(&self) -> &str;
    fn closed(&self) -> watch::Receiver<Option<String>>;
    fn close(&self, reason: &str);
    fn subscribe(&self, filter: EventFilter) -> EventStream;
    async fn send_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Reply;
    async fn send_guarded_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
        can_dispatch: DispatchGate,
    ) -> Reply {
        if !can_dispatch() {
            return Err(CdpError::Cancelled("dispatch authorization revoked".into()));
        }
        self.send_to(session_id, method, params, deadline).await
    }
    async fn send_batch_to(
        &self,
        session_id: Option<&str>,
        commands: Vec<(String, Value)>,
        deadline: Duration,
    ) -> Result<Vec<Value>, CdpError> {
        futures_util::future::try_join_all(
            commands.into_iter().map(|(method, params)| async move {
                self.send_to(session_id, &method, params, deadline).await
            }),
        )
        .await
    }
}

#[async_trait::async_trait]
impl CdpConnection for SocketConnection {
    fn endpoint(&self) -> &str {
        self.endpoint()
    }
    fn closed(&self) -> watch::Receiver<Option<String>> {
        self.closed()
    }
    fn close(&self, reason: &str) {
        self.close(reason);
    }
    fn subscribe(&self, filter: EventFilter) -> EventStream {
        self.subscribe(filter)
    }
    async fn send_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Reply {
        self.send_to(session_id, method, params, deadline).await
    }
    async fn send_guarded_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
        can_dispatch: DispatchGate,
    ) -> Reply {
        SocketConnection::send_guarded_to(self, session_id, method, params, deadline, can_dispatch)
            .await
    }
    async fn send_batch_to(
        &self,
        session_id: Option<&str>,
        commands: Vec<(String, Value)>,
        deadline: Duration,
    ) -> Result<Vec<Value>, CdpError> {
        SocketConnection::send_batch_to(self, session_id, commands, deadline).await
    }
}

#[derive(Clone)]
pub struct CdpClient(Arc<dyn CdpConnection>);

impl CdpClient {
    pub(crate) fn same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub async fn connect(endpoint: &str) -> Result<Self, String> {
        Ok(Self::from_connection(Arc::new(
            SocketConnection::connect(endpoint).await?,
        )))
    }

    pub fn from_connection(connection: Arc<dyn CdpConnection>) -> Self {
        Self(connection)
    }
    pub fn endpoint(&self) -> &str {
        self.0.endpoint()
    }
    pub fn closed(&self) -> watch::Receiver<Option<String>> {
        self.0.closed()
    }
    pub fn close_reason(&self) -> Option<String> {
        self.closed().borrow().clone()
    }
    pub fn close(&self, reason: &str) {
        self.0.close(reason);
    }
    pub fn subscribe(&self, filter: EventFilter) -> EventStream {
        self.0.subscribe(filter)
    }
    pub async fn send(&self, method: &str, params: Value) -> Reply {
        self.send_to(None, method, params, COMMAND_DEADLINE).await
    }
    pub async fn send_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Reply {
        self.0.send_to(session_id, method, params, deadline).await
    }
    pub async fn send_guarded_to(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
        deadline: Duration,
        can_dispatch: DispatchGate,
    ) -> Reply {
        self.0
            .send_guarded_to(session_id, method, params, deadline, can_dispatch)
            .await
    }
    /// Flattened session: page commands share this socket and carry `sessionId`.
    pub async fn attach_page(&self, target_id: &str) -> Result<PageChannel, CdpError> {
        let result = self
            .send(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
            )
            .await?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| CdpError::Malformed("attachToTarget returned no sessionId".into()))?;
        Ok(PageChannel {
            client: self.clone(),
            session_id: session_id.to_string(),
            target_id: target_id.to_string(),
        })
    }
}

async fn write_loop(
    mut writer: futures_util::stream::SplitSink<Socket, Message>,
    mut messages: mpsc::Receiver<OutgoingBatch>,
    inner: Weak<Inner>,
) {
    let Some(mut closed) = inner.upgrade().map(|inner| inner.closed.subscribe()) else {
        return;
    };
    loop {
        let batch = tokio::select! {
            _ = closed.wait_for(Option::is_some) => break,
            batch = messages.recv() => match batch {
                Some(batch) => batch,
                None => break,
            },
        };
        if batch.gate.is_some() {
            if let Some(owner) = inner.upgrade() {
                if !owner.should_dispatch(&batch) {
                    owner.cancel_pending(&batch.ids, "dispatch authorization revoked");
                    continue;
                }
            }
        }
        let written = async {
            for part in batch.messages {
                writer.feed(part).await?;
            }
            writer.flush().await
        }
        .await;
        if let Err(error) = written {
            log::warn!("[browser-cdp] write error (domios -> chrome): {error}");
            if let Some(inner) = inner.upgrade() {
                inner.close(&format!("CDP write error (domios -> chrome): {error}"));
            }
            break;
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), writer.close()).await;
}

async fn read_loop(mut reader: futures_util::stream::SplitStream<Socket>, inner: Weak<Inner>) {
    let Some(mut closed) = inner.upgrade().map(|inner| inner.closed.subscribe()) else {
        return;
    };
    let reason = loop {
        let message = tokio::select! {
            _ = closed.wait_for(Option::is_some) => break None,
            message = reader.next() => message,
        };
        let Some(owner) = inner.upgrade() else {
            break None;
        };
        match message {
            Some(Ok(Message::Text(text))) => {
                let document = response_id(&text).ok().flatten().is_some_and(|id| {
                    owner
                        .state
                        .lock()
                        .pending
                        .get(&id)
                        .is_some_and(|reply| reply.document)
                });
                let decoded = if document {
                    decode_document_incoming(text.to_string()).await
                } else {
                    decode_incoming(&text)
                };
                match decoded {
                    Ok(incoming) => owner.route(incoming).await,
                    Err(error) => {
                        log::warn!("[browser-cdp] undecodable message (chrome -> domios): {error}");
                        match response_id(&text) {
                            Ok(Some(id)) => {
                                let reply = owner.state.lock().pending.remove(&id);
                                if let Some(reply) = reply {
                                    let _ = reply.sender.send(Err(CdpError::Malformed(format!(
                                        "response {id} could not be decoded: {error}"
                                    ))));
                                }
                            }
                            Ok(None) => {}
                            Err(envelope_error) => break Some(envelope_error),
                        }
                    }
                }
            }
            Some(Ok(Message::Ping(payload))) => {
                let _ = owner
                    .outgoing
                    .try_send(OutgoingBatch::plain(vec![Message::Pong(payload)]));
            }
            Some(Ok(Message::Close(frame))) => {
                break Some(format!("chrome closed the CDP socket ({frame:?})"));
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => {
                if matches!(error, WsError::Capacity(_)) {
                    log::error!("[browser-cdp] message over {MAX_MESSAGE_SIZE} bytes (chrome -> domios): {error}");
                } else {
                    log::warn!("[browser-cdp] read error (chrome -> domios): {error}");
                }
                break Some(format!("CDP read error (chrome -> domios): {error}"));
            }
            None => break Some("CDP socket ended (chrome -> domios)".to_string()),
        }
    };
    if let (Some(reason), Some(inner)) = (reason, inner.upgrade()) {
        inner.close(&reason);
    }
}

/// Dropping the stream unsubscribes it.
pub struct EventStream {
    dropped: Arc<AtomicU64>,
    receiver: mpsc::Receiver<Arc<CdpEvent>>,
    id: u64,
    client: Weak<Inner>,
}

impl EventStream {
    pub fn has_lost_events(&self) -> bool {
        self.dropped.load(Ordering::Relaxed) != 0
    }
    pub fn try_recv(&mut self) -> Result<Arc<CdpEvent>, mpsc::error::TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn is_closed(&self) -> bool {
        self.receiver.is_closed()
    }
    pub fn from_receiver(receiver: mpsc::Receiver<Arc<CdpEvent>>) -> Self {
        Self {
            dropped: Arc::new(AtomicU64::new(0)),
            receiver,
            id: 0,
            client: Weak::new(),
        }
    }

    /// `None` once the connection closed.
    pub async fn recv(&mut self) -> Option<Arc<CdpEvent>> {
        self.receiver.recv().await
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        if let Some(inner) = self.client.upgrade() {
            inner
                .state
                .lock()
                .subscribers
                .retain(|subscriber| subscriber.id != self.id);
        }
    }
}

/// A CDP handle bound to one page session: the only surface a plugin is granted.
#[derive(Clone)]
pub struct PageChannel {
    client: CdpClient,
    session_id: String,
    target_id: String,
}

impl PageChannel {
    pub fn same_page(&self, other: &Self) -> bool {
        self.client.same_connection(&other.client)
            && self.session_id == other.session_id
            && self.target_id == other.target_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn client(&self) -> &CdpClient {
        &self.client
    }

    pub async fn send(&self, method: &str, params: Value) -> Reply {
        self.send_with_deadline(method, params, COMMAND_DEADLINE)
            .await
    }

    pub async fn send_batch(&self, commands: Vec<(&str, Value)>) -> Result<Vec<Value>, CdpError> {
        self.client
            .0
            .send_batch_to(
                Some(&self.session_id),
                commands
                    .into_iter()
                    .map(|(method, params)| (method.to_string(), params))
                    .collect(),
                COMMAND_DEADLINE,
            )
            .await
    }

    pub async fn send_with_deadline(
        &self,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Reply {
        self.client
            .send_to(Some(&self.session_id), method, params, deadline)
            .await
    }

    pub async fn send_guarded_with_deadline(
        &self,
        method: &str,
        params: Value,
        deadline: Duration,
        can_dispatch: DispatchGate,
    ) -> Reply {
        self.client
            .send_guarded_to(
                Some(&self.session_id),
                method,
                params,
                deadline,
                can_dispatch,
            )
            .await
    }

    pub async fn send_browser(&self, method: &str, params: Value) -> Reply {
        self.client.send(method, params).await
    }

    /// Page-session events plus browser-level events (Target.*), like the POC page channel.
    pub fn subscribe(&self, methods: &[&str]) -> EventStream {
        self.client.subscribe(
            EventFilter {
                session: SessionFilter::SessionOrBrowser(self.session_id.clone()),
                methods: Vec::new(),
            }
            .with_methods(methods),
        )
    }

    pub fn subscribe_session(&self, methods: &[&str]) -> EventStream {
        self.client.subscribe(
            EventFilter {
                session: SessionFilter::Session(self.session_id.clone()),
                methods: Vec::new(),
            }
            .with_methods(methods),
        )
    }
}

#[cfg(test)]
mod tests;
