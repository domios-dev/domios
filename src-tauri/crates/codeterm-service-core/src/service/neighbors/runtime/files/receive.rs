use super::*;
use async_trait::async_trait;
use codeterm_neighbors::{files::transfer::ReceivePorts, wire::InboxMessage};

#[cfg(test)]
mod tests;

pub(in crate::service::neighbors::runtime) async fn prefetch(
    session: &listener::Session,
) -> codeterm_neighbors::Result<()> {
    use codeterm_neighbors::{keys::RecipientKeys, receive::InboxEvent, supervisor::InboxSession};
    session.current()?;
    let cursor = session.metadata.load(&session.friendship).await?;
    let mut inbox = session
        .transport
        .listen_from_cursor(&session.friendship, cursor.as_ref())
        .await?;
    let lifecycle = InstallationKeys {
        store: &OsKeyStore,
        registry: session.registry.as_ref(),
        scope: session.scope.clone(),
        uid: &session.transport.uid,
    };
    loop {
        let event = tokio::select! {
            biased;
            _ = session.stopped() => return Err(Error::Revoked),
            result = inbox.next() => result?,
        };
        session.current()?;
        match event {
            Some(InboxEvent::Ready) => {}
            Some(InboxEvent::Messages(mut messages)) => {
                messages
                    .sort_by(|a, b| (a.doc.received_at, &a.id).cmp(&(b.doc.received_at, &b.id)));
                let peers = session.transport.peer_keys(&session.friendship).await?;
                for message in messages {
                    let history_generation = session.service.neighbor_history_generation();
                    session.current()?;
                    if message.doc.envelope.friendship_id != session.friendship {
                        return Err(Error::WrongPeer);
                    }
                    let peer = match session.transport.verify(
                        &message.doc,
                        &peers,
                        &session.transport.uid,
                        super::super::now(),
                    ) {
                        Ok(peer) => peer,
                        Err(_) => continue,
                    };
                    let plaintext = match lifecycle.decrypt_for(&message.doc.envelope, peer).await {
                        Ok(plaintext) => plaintext,
                        Err(Error::Decryption | Error::InvalidEnvelope | Error::InvalidKey) => {
                            continue
                        }
                        Err(error) => return Err(error),
                    };
                    if message.id
                        != format!(
                            "{}_{}",
                            message.doc.envelope.sender_uid, message.doc.envelope.message_id
                        )
                    {
                        return Err(Error::WrongPeer);
                    }
                    if matches!(
                        message.doc.envelope.kind,
                        codeterm_neighbors::wire::Kind::Answer
                            | codeterm_neighbors::wire::Kind::Refusal
                    ) {
                        let gateway = super::super::gateway::Gateway {
                            service: session.service.clone(),
                            metadata: session.metadata.clone(),
                            friendship: session.friendship.clone(),
                            tab: session.tab.clone().unwrap_or_default(),
                            reply_target: None,
                            generation: session.generation,
                            history_generation,
                            stopped: session.stopped.clone(),
                        };
                        use codeterm_neighbors::receive::NeighborGateway;
                        match gateway.deliver(&message, plaintext).await? {
                            codeterm_neighbors::receive::GatewayReceipt::Delivered => {
                                session.current()?;
                                codeterm_neighbors::receive::acknowledge_inbound(
                                    session.transport.as_ref(),
                                    &gateway,
                                    &message,
                                )
                                .await?;
                            }
                            codeterm_neighbors::receive::GatewayReceipt::SubmitUnconfirmed => {
                                return Err(Error::Gateway)
                            }
                        }
                        // Pending questions keep their original cursor until a concierge admits them.
                        continue;
                    }
                    match session
                        .service
                        .receive_neighbor_files(
                            &session.metadata,
                            &message,
                            plaintext,
                            session.generation,
                        )
                        .await
                    {
                        Ok(visible) => {
                            session.current()?;
                            session
                                .service
                                .append_neighbor_history(
                                    &session.metadata,
                                    &session.friendship,
                                    super::super::super::handling::incoming(
                                        &message,
                                        &visible,
                                        Some("concierge_missing"),
                                    ),
                                    history_generation,
                                )
                                .await?;
                        }
                        Err(Error::InvalidEnvelope | Error::MessageTooLarge { .. }) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            None => return Err(Error::Transport),
        }
    }
}

impl ServiceContext {
    pub(in crate::service::neighbors::runtime) async fn receive_neighbor_files(
        &self,
        metadata: &Metadata,
        message: &InboxMessage,
        plaintext: Zeroizing<String>,
        listener_generation: u64,
    ) -> codeterm_neighbors::Result<Zeroizing<String>> {
        let envelope = &message.doc.envelope;
        let Some(body) = AttachmentBody::parse(&plaintext, &binding(envelope))? else {
            return Ok(plaintext);
        };
        let visible = Zeroizing::new(body.agent_text());
        let (transport, _, _, _) = self.neighbor_ports().await.map_err(|_| Error::Transport)?;
        for manifest in &body.attachments {
            let id = format!(
                "{}:{}:{}",
                envelope.recipient_uid, envelope.sender_uid, manifest.file_id
            );
            let lock = {
                let mut transfers = self.file_cell().transfers.lock();
                transfers.retain(|_, weak| weak.strong_count() > 0);
                match transfers.get(&id).and_then(std::sync::Weak::upgrade) {
                    Some(lock) => lock,
                    None => {
                        let lock = Arc::new(tokio::sync::Mutex::new(()));
                        transfers.insert(id, Arc::downgrade(&lock));
                        lock
                    }
                }
            };
            let _transfer = lock.lock().await;
            let ports = Ports {
                service: self,
                metadata,
                transport: transport.as_ref(),
                envelope,
                listener_generation,
            };
            ports.current()?;
            let entries = metadata.file_entries(&envelope.friendship_id).await?;
            if entries.iter().any(|entry| {
                entry.file_id == manifest.file_id
                    && entry.direction == "in"
                    && matches!(entry.state.as_str(), "saved" | "failed" | "expired")
            }) {
                continue;
            }
            self.record_neighbor_file(
                metadata,
                &envelope.friendship_id,
                storage::Entry::new(manifest, envelope, "in", "downloading"),
            )
            .await?;
            let result =
                codeterm_neighbors::files::transfer::receive(&ports, manifest, &binding(envelope))
                    .await;
            if let Err(error) = result {
                ports.current()?;
                let already_saved = metadata
                    .file_entries(&envelope.friendship_id)
                    .await?
                    .iter()
                    .any(|entry| {
                        entry.file_id == manifest.file_id
                            && entry.direction == "in"
                            && entry.state == "saved"
                    });
                if !already_saved {
                    let state = if matches!(
                        error,
                        Error::File(files::refusal::Failure {
                            reason: files::refusal::Reason::FileNotFound,
                            ..
                        })
                    ) {
                        "expired"
                    } else {
                        "failed"
                    };
                    self.record_neighbor_file(
                        metadata,
                        &envelope.friendship_id,
                        storage::Entry::new(manifest, envelope, "in", state),
                    )
                    .await?;
                }
                if matches!(
                    error,
                    Error::Revoked | Error::SessionRevoked | Error::NotReceiver | Error::Disabled
                ) {
                    return Err(error);
                }
            }
        }
        Ok(visible)
    }
}

struct Ports<'a> {
    service: &'a ServiceContext,
    metadata: &'a Metadata,
    transport: &'a FirebaseTransport,
    envelope: &'a Envelope,
    listener_generation: u64,
}

impl Ports<'_> {
    fn current(&self) -> codeterm_neighbors::Result<()> {
        self.service.require_neighbors_enabled()?;
        self.service.require_neighbor_receiver(None)?;
        if self
            .service
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire)
            != self.listener_generation
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }
}

#[async_trait]
impl ReceivePorts for Ports<'_> {
    async fn download(&self, manifest: &Manifest) -> codeterm_neighbors::Result<Vec<u8>> {
        self.current()?;
        let result = self.transport.calls.invoke("friendsFileDownload", json!({"installation_id":self.transport.installation_id,"file_id":manifest.file_id,"sender_uid":self.envelope.sender_uid})).await?;
        self.current()?;
        if result["schema_version"] != 1
            || result["file_id"] != manifest.file_id
            || result["ciphertext_size"] != manifest.ciphertext_size
            || result["ciphertext_sha256"] != manifest.ciphertext_sha256
        {
            return Err(Error::Decryption);
        }
        let encoded = result["ciphertext_b64url"]
            .as_str()
            .ok_or(Error::Transport)?
            .to_owned();
        let size = manifest.ciphertext_size;
        tokio::task::spawn_blocking(move || {
            codeterm_neighbors::wire::decode_url(&encoded, size, size)
        })
        .await
        .map_err(|_| Error::Decryption)?
    }
    async fn verify(
        &self,
        manifest: &Manifest,
        _: &Binding<'_>,
        object: Vec<u8>,
    ) -> codeterm_neighbors::Result<Zeroizing<Vec<u8>>> {
        self.current()?;
        let envelope = self.envelope.clone();
        let manifest = manifest.clone();
        let plaintext =
            tokio::task::spawn_blocking(move || manifest.open(&binding(&envelope), &object))
                .await
                .map_err(|_| Error::Decryption)??;
        self.current()?;
        Ok(plaintext)
    }
    async fn save(
        &self,
        manifest: &Manifest,
        plaintext: Zeroizing<Vec<u8>>,
    ) -> codeterm_neighbors::Result<()> {
        self.current()?;
        let entry = storage::Entry::new(manifest, self.envelope, "in", "saved");
        let metadata = self.metadata.clone();
        let friendship = self.envelope.friendship_id.clone();
        let folder = tokio::task::spawn_blocking(move || metadata.friend_folder(&friendship))
            .await
            .map_err(|_| Error::CursorStore)??;
        self.current()?;
        storage::save(folder, entry.clone(), plaintext).await?;
        self.current()?;
        self.service
            .record_neighbor_file(self.metadata, &self.envelope.friendship_id, entry)
            .await
    }
    async fn confirm(
        &self,
        manifest: &Manifest,
        outcome: &'static str,
    ) -> codeterm_neighbors::Result<()> {
        self.current()?;
        let result = self.transport.calls.invoke("friendsFileConfirm", json!({"installation_id":self.transport.installation_id,"file_id":manifest.file_id,"sender_uid":self.envelope.sender_uid,"outcome":outcome})).await?;
        if result["schema_version"] != 1
            || result["file_id"] != manifest.file_id
            || result["state"] != "deleted"
        {
            return Err(Error::Transport);
        }
        self.current()
    }
}
