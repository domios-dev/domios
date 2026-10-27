use super::*;

pub struct ControlListener {
    stream: tonic::Streaming<fs::ListenResponse>,
    _requests: tokio::sync::mpsc::Sender<fs::ListenRequest>,
}

impl FirebaseTransport {
    pub async fn listen_control(&self) -> Result<ControlListener> {
        let (client, token, database) = connection(self).await?;
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        sender
            .send(fs::ListenRequest {
                database: database.clone(),
                target_change: Some(fs::listen_request::TargetChange::AddTarget(fs::Target {
                    target_type: Some(fs::target::TargetType::Documents(
                        fs::target::DocumentsTarget {
                            documents: ["neighborAccounts"]
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
            &token,
            &database,
            tokio_stream::wrappers::ReceiverStream::new(receiver),
        )?;
        let stream = client
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
    pub async fn changed(&mut self) -> Result<Option<()>> {
        loop {
            let Some(response) = self.stream.message().await.map_err(|_| Error::Transport)? else {
                return Ok(None);
            };
            use fs::listen_response::ResponseType;
            match response.response_type {
                Some(
                    ResponseType::DocumentChange(_)
                    | ResponseType::DocumentDelete(_)
                    | ResponseType::DocumentRemove(_),
                ) => return Ok(Some(())),
                Some(ResponseType::TargetChange(change)) => {
                    target_checkpoint(&change)?;
                }
                Some(ResponseType::Filter(_)) | None => return Err(Error::Transport),
            }
        }
    }
}
