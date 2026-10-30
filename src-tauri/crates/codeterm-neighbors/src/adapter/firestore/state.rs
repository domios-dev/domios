use super::{authenticated_request, code_failure, connection, fs, rpc_failure, target_checkpoint, FirebaseTransport};
use crate::{state::{Snapshot, StateEvent, StateListener}, Error, Result};
use serde_json::Value;

pub(in crate::adapter) async fn listen(transport: &FirebaseTransport) -> Result<Box<dyn StateListener>> {
    let (mut client, token, database) = connection(transport).await?;
    let path = format!("{database}/documents/neighborState/{}", transport.uid);
    let (requests, receiver) = tokio::sync::mpsc::channel(1);
    requests.send(fs::ListenRequest {
        database: database.clone(),
        target_change: Some(fs::listen_request::TargetChange::AddTarget(fs::Target {
            target_type: Some(fs::target::TargetType::Documents(fs::target::DocumentsTarget { documents: vec![path.clone()] })),
            target_id: 1, ..Default::default()
        })), ..Default::default()
    }).await.map_err(|_| Error::Transport)?;
    let request = authenticated_request(&token, &database, tokio_stream::wrappers::ReceiverStream::new(receiver))?;
    let stream = client.listen(request).await.map_err(|status| rpc_failure(transport.calls.as_ref(), "state.listen", &path, status))?.into_inner();
    Ok(Box::new(Listener { stream, _requests: requests, calls: transport.calls.clone(), membership: Default::default(),
        state: DocumentState { path, pending: None, missing: true } }))
}

struct Listener {
    stream: tonic::Streaming<fs::ListenResponse>,
    _requests: tokio::sync::mpsc::Sender<fs::ListenRequest>,
    calls: std::sync::Arc<dyn super::AuthenticatedCall>,
    membership: super::watch::Membership,
    state: DocumentState,
}

#[async_trait::async_trait]
impl StateListener for Listener {
    async fn next(&mut self) -> Result<Option<StateEvent>> {
        loop {
            let Some(response) = self.stream.message().await.map_err(|status| rpc_failure(self.calls.as_ref(), "state.listen.read", &self.state.path, status))? else { return Ok(None); };
            if let Some(fs::listen_response::ResponseType::TargetChange(change)) = &response.response_type {
                if let Some(cause) = &change.cause {
                    return Err(code_failure(self.calls.as_ref(), "state.listen.target", &self.state.path, cause.code));
                }
            }
            self.membership.observe(&response)?;
            if let Some(snapshot) = self.state.observe(response)? { return Ok(Some(snapshot)); }
        }
    }
}

struct DocumentState { path: String, pending: Option<Snapshot>, missing: bool }
impl DocumentState {
    fn observe(&mut self, response: fs::ListenResponse) -> Result<Option<StateEvent>> {
        use fs::listen_response::ResponseType;
        match response.response_type {
            Some(ResponseType::DocumentChange(change)) if change.target_ids.contains(&1) => {
                let document = change.document.ok_or(Error::InvalidEnvelope)?;
                if document.name != self.path { return Err(Error::WrongPeer); }
                let fields = document.fields.into_iter().map(|(key, value)| Ok((key, decode(value, 0)?))).collect::<Result<serde_json::Map<_, _>>>()?;
                self.pending = Some(Snapshot::parse(Value::Object(fields))?);
                self.missing = false;
            }
            Some(ResponseType::DocumentDelete(change)) => { self.remove(&change.document)?; }
            Some(ResponseType::DocumentRemove(change)) => { self.remove(&change.document)?; }
            Some(ResponseType::TargetChange(change)) => {
                let checkpoint = target_checkpoint(&change)?;
                if checkpoint.ready || checkpoint.flush {
                    if self.missing { return Ok(Some(StateEvent::Missing)); }
                    return Ok(self.pending.take().map(StateEvent::Snapshot));
                }
            }
            Some(ResponseType::Filter(_)) | Some(ResponseType::DocumentChange(_)) => {}
            None => return Err(Error::ListenProtocol { kind: "response_missing" }),
        }
        Ok(None)
    }
    fn remove(&mut self, path: &str) -> Result<()> {
        if path != self.path { return Err(Error::WrongPeer); }
        self.pending = None; self.missing = true; Ok(())
    }
}

fn decode(value: fs::Value, depth: usize) -> Result<Value> {
    if depth > 5 { return Err(Error::InvalidEnvelope); }
    use fs::value::ValueType;
    match value.value_type {
        Some(ValueType::StringValue(value)) if value.len() <= 2048 => Ok(value.into()),
        Some(ValueType::IntegerValue(value)) if value >= 0 && value as u64 <= crate::wire::MAX_SAFE_INTEGER => Ok(value.into()),
        Some(ValueType::BooleanValue(value)) => Ok(value.into()),
        Some(ValueType::NullValue(_)) => Ok(Value::Null),
        Some(ValueType::ArrayValue(value)) if value.values.len() <= 200 => value.values.into_iter().map(|value| decode(value, depth + 1)).collect::<Result<Vec<_>>>().map(Value::Array),
        Some(ValueType::MapValue(value)) if value.fields.len() <= 24 => value.fields.into_iter().map(|(key, value)| Ok((key, decode(value, depth + 1)?))).collect::<Result<serde_json::Map<_, _>>>().map(Value::Object),
        _ => Err(Error::InvalidEnvelope),
    }
}

#[cfg(test)]
mod tests;
