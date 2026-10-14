use super::model::RuntimeFailure;
use codeterm_plugin::agent_runtime::{RpcCall, RpcFailure, RpcId, RuntimeFrame};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{broadcast, mpsc, oneshot, watch},
};

pub type Reply = Result<Value, RpcFailure>;
type Pending = oneshot::Sender<Result<Reply, RuntimeFailure>>;

pub(crate) type PrewriteGuard = Arc<dyn Fn() -> Result<(), RuntimeFailure> + Send + Sync>;

struct Write {
    text: String,
    guard: Option<PrewriteGuard>,
    written: oneshot::Sender<Result<(), RuntimeFailure>>,
}

enum WireFrame {
    Event(RuntimeFrame),
    Response { id: RpcId, result: Reply },
}

struct State {
    closed: Option<RuntimeFailure>,
    pending: HashMap<RpcId, Pending>,
}
struct Inner {
    state: Mutex<State>,
    next_id: AtomicI64,
    outgoing: mpsc::Sender<Write>,
    events: broadcast::Sender<RuntimeFrame>,
    initial_events: Mutex<Option<broadcast::Receiver<RuntimeFrame>>>,
    ended: watch::Sender<Option<RuntimeFailure>>,
}

#[derive(Clone)]
pub struct Client(Arc<Inner>);

impl Client {
    pub fn attach<R, W>(reader: R, mut writer: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outgoing, mut messages) = mpsc::channel::<Write>(64);
        let (events, initial_events) = broadcast::channel(512);
        let (ended, _) = watch::channel(None);
        let client = Self(Arc::new(Inner {
            state: Mutex::new(State {
                closed: None,
                pending: HashMap::new(),
            }),
            next_id: AtomicI64::new(1),
            outgoing,
            events,
            initial_events: Mutex::new(Some(initial_events)),
            ended,
        }));
        let sender = client.clone();
        let mut ended = client.ended();
        tauri::async_runtime::spawn(async move {
            loop {
                let text = tokio::select! {
                    _ = ended.changed() => break,
                    text = messages.recv() => match text {Some(text)=>text,None=>break},
                };
                if let Some(reason) = text.guard.as_ref().and_then(|guard| guard().err()) {
                    let _ = text
                        .written
                        .send(Err(RuntimeFailure::PrewriteRefused(reason.to_string())));
                    continue;
                }
                let sent = tokio::select! {
                    _ = ended.changed() => break,
                    sent = tokio::time::timeout(Duration::from_secs(15), async {
                        writer.write_all(text.text.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await
                    }) => sent,
                };
                match sent {
                    Ok(Ok(())) => {
                        let _ = text.written.send(Ok(()));
                    }
                    Ok(Err(error)) => {
                        sender.close(RuntimeFailure::PipeFailure {
                            stream: "stdin".into(),
                            reason: error.to_string(),
                        });
                        break;
                    }
                    Err(_) => {
                        sender.close(RuntimeFailure::Deadline);
                        break;
                    }
                }
            }
        });
        let receiver = client.clone();
        let mut ended = client.ended();
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(reader);
            loop {
                let text = tokio::select! { _ = ended.changed() => break, text = read_frame(&mut reader) => text };
                let frame = match text
                    .and_then(|text| text.ok_or(RuntimeFailure::PipeEof))
                    .and_then(|text| decode(&text))
                {
                    Ok(frame) => frame,
                    Err(error) => {
                        receiver.close(error);
                        break;
                    }
                };
                if let WireFrame::Response { id, result } = frame {
                    if let Some(reply) = receiver.0.state.lock().pending.remove(&id) {
                        let _ = reply.send(Ok(result));
                    }
                } else if let WireFrame::Event(frame) = frame {
                    let _ = receiver.0.events.send(frame);
                }
            }
        });
        client
    }

    pub fn ended(&self) -> watch::Receiver<Option<RuntimeFailure>> {
        self.0.ended.subscribe()
    }
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeFrame> {
        self.0
            .initial_events
            .lock()
            .take()
            .unwrap_or_else(|| self.0.events.subscribe())
    }
    pub fn close(&self, reason: RuntimeFailure) {
        let pending = {
            let mut state = self.0.state.lock();
            if state.closed.is_some() {
                return;
            }
            state.closed = Some(reason.clone());
            std::mem::take(&mut state.pending)
        };
        for (_, reply) in pending {
            let _ = reply.send(Err(reason.clone()));
        }
        self.0.ended.send_replace(Some(reason));
    }

    pub(crate) async fn request_guarded(
        &self,
        call: &RpcCall,
        guard: Option<PrewriteGuard>,
    ) -> Result<Reply, RuntimeFailure> {
        let id = RpcId::Number(self.0.next_id.fetch_add(1, Ordering::Relaxed));
        let (reply, received) = oneshot::channel();
        {
            let mut state = self.0.state.lock();
            if let Some(reason) = &state.closed {
                return Err(reason.clone());
            }
            state.pending.insert(id.clone(), reply);
        }
        let pending = PendingRequest {
            client: self.clone(),
            id: id.clone(),
        };
        self.queue_guarded(
            json!({"id":id,"method":call.method,"params":call.params}),
            guard,
        )
        .await?;
        let result = tokio::time::timeout(Duration::from_secs(30), received)
            .await
            .map_err(|_| RuntimeFailure::Deadline)?
            .map_err(|_| RuntimeFailure::PipeEof)?;
        drop(pending);
        result
    }

    pub async fn notify(&self, call: &RpcCall) -> Result<(), RuntimeFailure> {
        self.queue(json!({"method":call.method,"params":call.params}))
            .await
    }
    pub async fn reply(&self, id: &RpcId, result: Reply) -> Result<(), RuntimeFailure> {
        self.queue(match result {
            Ok(result) => json!({"id":id,"result":result}),
            Err(error) => json!({"id":id,"error":error}),
        })
        .await
    }
    async fn queue(&self, frame: Value) -> Result<(), RuntimeFailure> {
        self.queue_guarded(frame, None).await
    }
    async fn queue_guarded(
        &self,
        frame: Value,
        guard: Option<PrewriteGuard>,
    ) -> Result<(), RuntimeFailure> {
        if let Some(reason) = self.0.state.lock().closed.clone() {
            return Err(reason);
        }
        let text = frame.to_string();
        if text.len() > FRAME_LIMIT {
            return Err(RuntimeFailure::InvalidFrame("Frame size limit".into()));
        }
        let (written, receipt) = oneshot::channel();
        self.0
            .outgoing
            .try_send(Write {
                text,
                guard,
                written,
            })
            .map_err(|error| RuntimeFailure::PipeFailure {
                stream: "stdin".into(),
                reason: error.to_string(),
            })?;
        tokio::time::timeout(Duration::from_secs(15), receipt)
            .await
            .map_err(|_| RuntimeFailure::Deadline)?
            .map_err(|_| {
                self.0
                    .state
                    .lock()
                    .closed
                    .clone()
                    .unwrap_or(RuntimeFailure::PipeEof)
            })?
    }
}

struct PendingRequest {
    client: Client,
    id: RpcId,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.client.0.state.lock().pending.remove(&self.id);
    }
}

const FRAME_LIMIT: usize = 4 * 1024 * 1024;
async fn read_frame<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<String>, RuntimeFailure> {
    let mut frame = Vec::new();
    loop {
        let bytes = reader
            .fill_buf()
            .await
            .map_err(|error| RuntimeFailure::PipeFailure {
                stream: "stdout".into(),
                reason: error.to_string(),
            })?;
        if bytes.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                String::from_utf8(frame)
                    .map(Some)
                    .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))
            };
        }
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |end| end + 1);
        if frame.len() + count > FRAME_LIMIT {
            return Err(RuntimeFailure::InvalidFrame("Frame size limit".into()));
        }
        frame.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            return String::from_utf8(frame)
                .map(Some)
                .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()));
        }
    }
}

fn decode(text: &str) -> Result<WireFrame, RuntimeFailure> {
    let parsed: Value = serde_json::from_str(text)
        .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))?;
    let value = parsed
        .as_object()
        .ok_or_else(|| RuntimeFailure::InvalidFrame("Frame must be an object".into()))?;
    let id = value
        .get("id")
        .map(|id| serde_json::from_value(id.clone()))
        .transpose()
        .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))?;
    if value.get("jsonrpc").is_some_and(|version| version != "2.0")
        || id.as_ref().is_some_and(|id| matches!(id, RpcId::Number(number) if number.unsigned_abs() > 9_007_199_254_740_991))
    {
        return Err(RuntimeFailure::InvalidFrame("Unsupported version or unsafe ID".into()));
    }
    if let Some(method) = value.get("method") {
        if value.contains_key("result") || value.contains_key("error") {
            return Err(RuntimeFailure::InvalidFrame("Ambiguous request".into()));
        }
        let method = method
            .as_str()
            .ok_or_else(|| RuntimeFailure::InvalidFrame("Method must be a string".into()))?
            .to_string();
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        return Ok(match id {
            Some(request_id) => WireFrame::Event(RuntimeFrame::Request {
                request_id,
                method,
                params,
            }),
            None => WireFrame::Event(RuntimeFrame::Notification { method, params }),
        });
    }
    let id = id.ok_or_else(|| RuntimeFailure::InvalidFrame("Response id missing".into()))?;
    let result = match (value.get("result"), value.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => Err(serde_json::from_value(error.clone())
            .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))?),
        _ => return Err(RuntimeFailure::InvalidFrame("Ambiguous response".into())),
    };
    Ok(WireFrame::Response { id, result })
}

#[cfg(test)]
mod tests;
