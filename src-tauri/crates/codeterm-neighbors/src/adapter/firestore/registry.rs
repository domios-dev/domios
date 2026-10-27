use super::{
    authenticated_request, code_failure, connection, fs, json, rpc_failure, target_checkpoint,
    FirebaseTransport,
};
use crate::adapter::AuthenticatedCall;
use crate::{
    registry::{Acceptance, RegistryListener, RegistrySignal},
    wire::{hex_id, uid},
    Error, Result,
};
use async_trait::async_trait;

fn document_path(database: &str, owner: &str) -> Result<String> {
    if !uid(owner) {
        return Err(Error::InvalidEnvelope);
    }
    Ok(format!("{database}/documents/neighborSignals/{owner}"))
}

pub(in crate::adapter) async fn listen(
    transport: &FirebaseTransport,
) -> Result<Box<dyn RegistryListener>> {
    let (mut client, token, database) = connection(transport).await?;
    let path = document_path(&database, &transport.uid)?;
    let (requests, receiver) = tokio::sync::mpsc::channel(1);
    requests
        .send(fs::ListenRequest {
            database: database.clone(),
            target_change: Some(fs::listen_request::TargetChange::AddTarget(fs::Target {
                target_type: Some(fs::target::TargetType::Documents(
                    fs::target::DocumentsTarget {
                        documents: vec![path.clone()],
                    },
                )),
                target_id: 1,
                ..Default::default()
            })),
            ..Default::default()
        })
        .await
        .map_err(|_| Error::Transport)?;
    let request = authenticated_request(
        &token,
        &database,
        tokio_stream::wrappers::ReceiverStream::new(receiver),
    )?;
    let stream = client
        .listen(request)
        .await
        .map_err(|status| rpc_failure(transport.calls.as_ref(), "registry.listen", &path, status))?
        .into_inner();
    Ok(Box::new(Listener {
        stream,
        _requests: requests,
        calls: transport.calls.clone(),
        state: SignalState {
            path,
            pending: RegistrySignal::default(),
            dirty: false,
        },
    }))
}

struct Listener {
    calls: std::sync::Arc<dyn AuthenticatedCall>,
    stream: tonic::Streaming<fs::ListenResponse>,
    _requests: tokio::sync::mpsc::Sender<fs::ListenRequest>,
    state: SignalState,
}

#[async_trait]
impl RegistryListener for Listener {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        loop {
            let Some(response) = self.stream.message().await.map_err(|status| {
                rpc_failure(
                    self.calls.as_ref(),
                    "registry.listen.read",
                    &self.state.path,
                    status,
                )
            })?
            else {
                return Ok(None);
            };
            if let Some(fs::listen_response::ResponseType::TargetChange(change)) =
                &response.response_type
            {
                if let Some(cause) = &change.cause {
                    return Err(code_failure(
                        self.calls.as_ref(),
                        "registry.listen.target",
                        &self.state.path,
                        cause.code,
                    ));
                }
            }
            if let Some(signal) = self.state.observe(response)? {
                return Ok(Some(signal));
            }
        }
    }
}

struct SignalState {
    path: String,
    pending: RegistrySignal,
    dirty: bool,
}

impl SignalState {
    fn observe(&mut self, response: fs::ListenResponse) -> Result<Option<RegistrySignal>> {
        use fs::listen_response::ResponseType;
        match response.response_type {
            Some(ResponseType::DocumentChange(change)) => {
                if !change.target_ids.contains(&1) {
                    return Ok(None);
                }
                let document = change.document.ok_or(Error::Transport)?;
                if document.name != self.path {
                    return Err(Error::WrongPeer);
                }
                self.pending = parse_signal(document)?;
                self.dirty = true;
            }
            Some(ResponseType::DocumentDelete(change)) => self.remove(&change.document)?,
            Some(ResponseType::DocumentRemove(change)) => self.remove(&change.document)?,
            Some(ResponseType::TargetChange(change)) => {
                let checkpoint = target_checkpoint(&change)?;
                if checkpoint.ready || checkpoint.flush && self.dirty {
                    self.dirty = false;
                    return Ok(Some(std::mem::take(&mut self.pending)));
                }
            }
            Some(ResponseType::Filter(_)) | None => return Err(Error::Transport),
        }
        Ok(None)
    }

    fn remove(&mut self, path: &str) -> Result<()> {
        if path != self.path {
            return Err(Error::WrongPeer);
        }
        self.pending = RegistrySignal::default();
        self.dirty = true;
        Ok(())
    }
}

fn parse_signal(document: fs::Document) -> Result<RegistrySignal> {
    let mut fields = document.fields;
    if fields.len() > 12 {
        return Err(Error::InvalidEnvelope);
    }
    let mut field = |key: &str| json(fields.remove(key).ok_or(Error::InvalidEnvelope)?, 0);
    if field("schema_version")? != 1 {
        return Err(Error::InvalidEnvelope);
    }
    let revision = field("revision")?;
    if revision.as_u64().is_none() {
        return Err(Error::InvalidEnvelope);
    }
    let event = field("event_id")?;
    let event_id = event
        .as_str()
        .filter(|id| hex_id(id, 32))
        .ok_or(Error::InvalidEnvelope)?;
    let kind = field("type")?;
    if !matches!(
        kind.as_str(),
        Some(
            "request_received"
                | "request_accepted"
                | "request_declined"
                | "friend_removed"
                | "receiver_changed"
        )
    ) {
        return Err(Error::InvalidEnvelope);
    }
    let request = field("request_id")?;
    let friendship = field("friendship_id")?;
    for id in [&request, &friendship] {
        if !id.is_null() && !id.as_str().is_some_and(|id| hex_id(id, 64)) {
            return Err(Error::InvalidEnvelope);
        }
    }
    let accepted = if kind == "request_accepted" {
        match (request.as_str(), friendship.as_str()) {
            (None, Some(friendship_id)) => vec![Acceptance {
                event_id: event_id.into(),
                friendship_id: friendship_id.into(),
            }],
            _ => vec![],
        }
    } else {
        vec![]
    };
    let receiver_revision = if kind == "receiver_changed" {
        Some(
            field("receiver_revision")?
                .as_u64()
                .ok_or(Error::InvalidEnvelope)?,
        )
    } else {
        None
    };
    Ok(RegistrySignal {
        accepted,
        receiver_revision,
    })
}

#[cfg(test)]
mod tests;
