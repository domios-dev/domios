use super::*;

pub enum ControlChange {
    Receiver,
    Friends,
}

pub struct ControlListener {
    stream: tonic::Streaming<fs::ListenResponse>,
    _requests: tokio::sync::mpsc::Sender<fs::ListenRequest>,
}

impl FirebaseTransport {
    pub async fn listen_control(&self) -> Result<ControlListener> {
        if !crate::wire::uid(&self.uid)
            || self.project.is_empty()
            || !self
                .project
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::InvalidEnvelope);
        }
        let channel = connect().await?;
        let database = format!("projects/{}/databases/(default)", self.project);
        let token = self.calls.id_token().await?;
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        sender
            .send(fs::ListenRequest {
                database: database.clone(),
                target_change: Some(fs::listen_request::TargetChange::AddTarget(fs::Target {
                    target_type: Some(fs::target::TargetType::Documents(
                        fs::target::DocumentsTarget {
                            documents: ["neighborAccounts", "neighborSignals"]
                                .iter()
                                .map(|collection| {
                                    format!("{database}/documents/{collection}/{}", self.uid)
                                })
                                .collect(),
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
            tokio_stream::wrappers::ReceiverStream::new(receiver),
            &token,
            &database,
        )?;
        let stream = FirestoreClient::new(channel)
            .max_decoding_message_size(128 * 1024)
            .listen(request)
            .await
            .map_err(|_| Error::Transport)?
            .into_inner();
        Ok(ControlListener {
            stream,
            _requests: sender,
        })
    }
}

impl ControlListener {
    pub async fn changed(&mut self) -> Result<Option<ControlChange>> {
        loop {
            let Some(response) = self.stream.message().await.map_err(|_| Error::Transport)? else {
                return Ok(None);
            };
            use fs::listen_response::ResponseType;
            match response.response_type {
                Some(ResponseType::DocumentChange(change)) => {
                    let document = change.document.ok_or(Error::Transport)?;
                    let receiver = document.name.contains("/neighborAccounts/") || document.fields.get("type").is_some_and(|value| matches!(&value.value_type, Some(fs::value::ValueType::StringValue(kind)) if kind == "receiver_changed"));
                    return Ok(Some(if receiver {
                        ControlChange::Receiver
                    } else {
                        ControlChange::Friends
                    }));
                }
                Some(ResponseType::DocumentDelete(_) | ResponseType::DocumentRemove(_)) => {
                    return Ok(Some(ControlChange::Receiver))
                }
                Some(ResponseType::TargetChange(change)) => {
                    target_checkpoint(&change)?;
                }
                Some(ResponseType::Filter(_)) | None => return Err(Error::Transport),
            }
        }
    }
}
