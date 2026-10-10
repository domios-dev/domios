use super::{
    buffer::NotificationBuffer,
    protocol::{self, Frame, RequestId, RpcError},
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

#[derive(Clone, Debug, PartialEq)]
pub enum ClientError {
    Closed(String),
    Rpc(RpcError),
    Deadline,
}

type Reply = Result<Value, ClientError>;

fn connection_error(error: tokio_tungstenite::tungstenite::Error) -> String {
    use std::io::ErrorKind;
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::ConnectionClosed | Error::AlreadyClosed => "RPC WebSocket connection failed".into(),
        Error::Io(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::NotConnected
                    | ErrorKind::BrokenPipe
                    | ErrorKind::TimedOut
                    | ErrorKind::UnexpectedEof
                    | ErrorKind::Interrupted
                    | ErrorKind::WouldBlock
            ) =>
        {
            "RPC WebSocket connection failed".into()
        }
        other => format!("RPC WebSocket handshake refused: {other}"),
    }
}

struct State {
    closed: Option<String>,
    pending: HashMap<RequestId, oneshot::Sender<Reply>>,
    buffer: NotificationBuffer,
    additional_context: HashMap<String, serde_json::Map<String, Value>>,
}

struct Inner {
    state: Mutex<State>,
    next_id: AtomicI64,
    initialized: AtomicBool,
    outgoing: Mutex<mpsc::Sender<Message>>,
    epoch: AtomicU64,
    resync: watch::Sender<u64>,
    events: broadcast::Sender<Frame>,
    observations: broadcast::Sender<Frame>,
    death: watch::Sender<Option<String>>,
}

#[derive(Clone)]
pub struct Client(Arc<Inner>);

impl Client {
    pub async fn connect(port: u16, token: &str) -> Result<Self, String> {
        let (outgoing, _) = mpsc::channel(64);
        let (events, _) = broadcast::channel(512);
        let (observations, _) = broadcast::channel(512);
        let (death, _) = watch::channel(None);
        let client = Self(Arc::new(Inner {
            state: Mutex::new(State {
                closed: None,
                pending: HashMap::new(),
                buffer: NotificationBuffer::new(128, 32),
                additional_context: HashMap::new(),
            }),
            next_id: AtomicI64::new(1),
            initialized: AtomicBool::new(false),
            outgoing: Mutex::new(outgoing),
            epoch: AtomicU64::new(0),
            resync: watch::channel(0).0,
            events,
            observations,
            death,
        }));
        client.reconnect(port, token).await?;
        Ok(client)
    }

    pub async fn reconnect(&self, port: u16, token: &str) -> Result<(), String> {
        let mut request = format!("ws://127.0.0.1:{port}")
            .into_client_request()
            .map_err(|error| error.to_string())?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| "Invalid RPC token")?,
        );
        let config = super::transport_config::websocket_config();

        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(15),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| "RPC WebSocket readiness deadline")?
        .map_err(connection_error)?;
        let (mut writer, mut reader) = socket.split();
        let (outgoing, mut messages) = mpsc::channel(64);
        let epoch = self.0.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        *self.0.outgoing.lock() = outgoing;
        self.0.state.lock().closed = None;
        self.0.initialized.store(false, Ordering::Release);
        self.0.death.send_replace(None);
        let sender = self.clone();
        let mut shutdown = self.0.death.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => break,
                    message = messages.recv() => match message {
                        Some(message) => {
                            let sent = tokio::select! {
                                _ = shutdown.changed() => break,
                                sent = tokio::time::timeout(Duration::from_secs(15), writer.send(message)) => sent,
                            };
                            if !matches!(sent, Ok(Ok(()))) { sender.close_epoch(epoch, "RPC writer exited"); break; }
                        },
                        None => break,
                    }
                }
            }
            let _ = tokio::time::timeout(Duration::from_secs(1), writer.close()).await;
        });
        let receiver = self.clone();
        let mut shutdown = self.0.death.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => break,
                    message = reader.next() => match message {
                        Some(Ok(Message::Text(text))) => match protocol::decode(&text) {
                            Ok(frame) => if receiver.0.epoch.load(Ordering::Acquire) != epoch { break; } else if let Err(error) = receiver.ingest(frame) { receiver.close_epoch(epoch, &error); break; },
                            Err(_) => { receiver.close_epoch(epoch, "Invalid RPC frame"); break; }
                        },
                        Some(Ok(Message::Ping(bytes))) => { let _ = receiver.0.outgoing.lock().try_send(Message::Pong(bytes)); },
                        Some(Ok(Message::Pong(_))) => {},
                        Some(Err(error)) => { receiver.close_epoch(epoch, &format!("RPC reader error: {error}")); break; },
                        _ => { receiver.close_epoch(epoch, "RPC reader exited"); break; }
                    }
                }
            }
        });
        Ok(())
    }

    fn close_epoch(&self, epoch: u64, reason: &str) {
        if self.0.epoch.load(Ordering::Acquire) == epoch {
            self.close(reason);
        }
    }

    pub fn resync(&self) -> watch::Receiver<u64> {
        self.0.resync.subscribe()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.0.events.subscribe()
    }
    pub fn observe(&self) -> broadcast::Receiver<Frame> {
        self.0.observations.subscribe()
    }
    pub fn death(&self) -> watch::Receiver<Option<String>> {
        self.0.death.subscribe()
    }

    pub fn close(&self, reason: &str) {
        let pending = {
            let mut state = self.0.state.lock();
            if state.closed.is_some() {
                return;
            }
            state.closed = Some(reason.into());
            std::mem::take(&mut state.pending)
        };
        for (_, reply) in pending {
            let _ = reply.send(Err(ClientError::Closed(reason.into())));
        }
        self.0.death.send_replace(Some(reason.into()));
    }

    fn ingest(&self, frame: Frame) -> Result<(), String> {
        if !matches!(&frame, Frame::Response { .. }) {
            let _ = self.0.observations.send(frame.clone());
        }
        if let Frame::Response { id, result } = frame {
            let reply = self.0.state.lock().pending.remove(&id);
            if let Some(reply) = reply {
                let _ = reply.send(result.map_err(ClientError::Rpc));
            }
        } else {
            let buffered = self.0.state.lock().buffer.accept(frame.clone());
            let frame = match buffered {
                Ok(frame) => frame,
                Err(_) => {
                    self.0
                        .resync
                        .send_modify(|generation| *generation = generation.wrapping_add(1));
                    if matches!(frame, Frame::ServerRequest { .. }) {
                        Some(frame)
                    } else {
                        None
                    }
                }
            };
            if let Some(frame) = frame {
                let _ = self.0.events.send(frame);
            }
        }
        Ok(())
    }

    pub fn release_thread(&self, thread: &str) {
        let mut state = self.0.state.lock();
        for frame in state.buffer.release(thread) {
            let _ = self.0.events.send(frame);
        }
    }

    pub fn additional_context(&self, thread: &str) -> serde_json::Map<String, Value> {
        self.0
            .state
            .lock()
            .additional_context
            .get(thread)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn initialize(&self) -> Reply {
        if self.0.initialized.swap(true, Ordering::SeqCst) {
            return Err(ClientError::Closed(
                "RPC initialize already attempted".into(),
            ));
        }
        let result = self
            .request("initialize", protocol::initialize_params())
            .await?;
        self.notify("initialized", json!({}))?;
        Ok(result)
    }

    pub async fn request(&self, method: &str, params: Value) -> Reply {
        if !matches!(
            method,
            "initialize"
                | "thread/start"
                | "thread/resume"
                | "thread/read"
                | "thread/unarchive"
                | "thread/inject_items"
                | "thread/turns/list"
                | "thread/items/list"
                | "account/read"
                | "model/list"
                | "turn/start"
                | "turn/steer"
                | "turn/interrupt"
                | "thread/compact/start"
                | "thread/goal/set"
                | "thread/goal/get"
        ) {
            return Err(ClientError::Closed(
                "Method is outside the sidecar contract".into(),
            ));
        }
        let id = RequestId::Number(self.0.next_id.fetch_add(1, Ordering::Relaxed));
        let (reply, receiver) = oneshot::channel();
        {
            let mut state = self.0.state.lock();
            if let Some(reason) = &state.closed {
                return Err(ClientError::Closed(reason.clone()));
            }
            if matches!(method, "turn/start" | "turn/steer") {
                if let (Some(thread), Some(context)) = (
                    params["threadId"].as_str(),
                    params["additionalContext"].as_object(),
                ) {
                    state
                        .additional_context
                        .entry(thread.into())
                        .or_default()
                        .extend(context.clone());
                }
            }
            state.pending.insert(id.clone(), reply);
        }
        let cleanup = PendingRequest {
            client: self.clone(),
            id: id.clone(),
        };
        self.0
            .outgoing
            .lock()
            .try_send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| ClientError::Closed("RPC outbound queue unavailable".into()))?;
        let result = tokio::time::timeout(Duration::from_secs(30), receiver)
            .await
            .map_err(|_| ClientError::Deadline)?
            .map_err(|_| ClientError::Closed("RPC response channel closed".into()))?;
        drop(cleanup);
        result
    }

    pub fn reply(
        &self,
        id: &RequestId,
        result: Result<Value, RpcError>,
    ) -> Result<(), ClientError> {
        let frame = match result {
            Ok(result) => json!({"id":id,"result":result}),
            Err(error) => {
                json!({"id":id,"error":{"code":error.code,"message":error.message,"data":error.data}})
            }
        };
        self.0
            .outgoing
            .lock()
            .try_send(Message::Text(frame.to_string().into()))
            .map_err(|_| ClientError::Closed("RPC reply queue unavailable".into()))
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), ClientError> {
        self.0
            .outgoing
            .lock()
            .try_send(Message::Text(
                json!({"method":method,"params":params}).to_string().into(),
            ))
            .map_err(|_| ClientError::Closed("RPC notification queue unavailable".into()))
    }
}

struct PendingRequest {
    client: Client,
    id: RequestId,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.client.0.state.lock().pending.remove(&self.id);
    }
}

#[cfg(test)]
mod tests;
