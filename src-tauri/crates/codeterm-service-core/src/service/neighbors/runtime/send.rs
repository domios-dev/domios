use super::*;

impl ServiceContext {
    pub(super) async fn send_neighbor(
        &self,
        transport: &FirebaseTransport,
        lifecycle: &InstallationKeys<'_>,
        metadata: &Metadata,
        outgoing: Outgoing,
    ) -> codeterm_neighbors::Result<Value> {
        let generation = self.neighbor_history_generation();
        let friendship = outgoing.friendship.clone();
        let agent = super::super::notifications::AgentNotice::for_author(
            &outgoing.author,
            outgoing.in_reply_to.is_some(),
        );
        let result = self
            .send_neighbor_once(transport, lifecycle, metadata, outgoing)
            .await;
        let rejection = match &result {
            Ok(result) => result.rejection,
            Err(error) => super::super::delivery::Rejection::from_error(*error),
        };
        if let (Some(agent), Some(rejection)) = (agent, rejection) {
            if let Err(error) = self
                .notify_neighbor_rejection(metadata, &friendship, agent, rejection, generation)
                .await
            {
                log::debug!("[neighbors] rejected-send notification unavailable: {error}");
            }
        }
        result.map(|result| result.receipt)
    }

    async fn send_neighbor_once(
        &self,
        transport: &FirebaseTransport,
        lifecycle: &InstallationKeys<'_>,
        metadata: &Metadata,
        outgoing: Outgoing,
    ) -> codeterm_neighbors::Result<super::super::delivery::SendResult> {
        self.require_neighbors_enabled()?;
        let generation = self.neighbor_history_generation();
        let Outgoing {
            friendship,
            kind,
            body,
            in_reply_to,
            author,
        } = outgoing;
        if !hex_id(&friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        let permission = metadata
            .settings()
            .await?
            .permissions
            .get(&friendship)
            .cloned()
            .unwrap_or_default();
        let (body, paths) = prepare_outgoing_body(
            metadata,
            &friendship,
            &permission,
            kind,
            body,
            in_reply_to.as_deref(),
        )
        .await?;
        let keys = lifecycle.load().await?.ok_or(Error::Revoked)?;
        if !lifecycle
            .registry
            .active_key_ids()
            .await?
            .contains(&keys.verifier_key_id(&transport.uid))
        {
            return Err(Error::Revoked);
        }
        let peers = transport.peer_keys(&friendship).await?;
        let peer = peers
            .keys
            .iter()
            .max_by_key(|key| key.created_at)
            .ok_or(Error::InvalidKey)?;
        let mut envelope = Envelope {
            schema: SCHEMA.into(),
            friendship_id: friendship.clone(),
            message_id: uuid::Uuid::new_v4().simple().to_string(),
            sender_uid: transport.uid.clone(),
            recipient_uid: peers.peer_uid,
            kind,
            in_reply_to: in_reply_to.clone(),
            ciphertext: String::new(),
            nonce: String::new(),
            enc_key_ids: EncryptionKeyIds {
                sender: String::new(),
                recipient: String::new(),
            },
            scope_sha256: permission.scope_sha256()?,
            verdict: "pass".into(),
            verifier_key_id: keys.verifier_key_id(&transport.uid),
            issued_at: now().seconds,
        };
        let (body, visible_body, attachments) = self
            .prepare_neighbor_files(transport, metadata, &envelope, body, paths)
            .await?;
        keys.encrypt(
            &mut envelope,
            &peer.encryption_public_key_spki_b64,
            &body,
            &mut rand::rngs::OsRng,
        )?;
        let signed = keys.sign(envelope)?;
        self.require_neighbor_history_generation(generation)?;
        self.append_neighbor_history(
            metadata,
            &friendship,
            super::super::history::Entry {
                received_by: None,
                flags: Vec::new(),
                flag_callers: Vec::new(),
                handling: Default::default(),
                direction: "out".into(),
                author,
                body: visible_body,
                sender_uid: transport.uid.clone(),
                at: now().seconds * 1000 + u64::from(now().nanoseconds / 1_000_000),
                message_id: signed.envelope.message_id.clone(),
                kind,
                in_reply_to,
                outcome: None,
                reason: None,
                limit: None,
                resets_at: None,
                retry_after_seconds: None,
                submission: None,
                delivered_at: None,
                read_at: None,
                delivery_status: Some(codeterm_neighbors::wire::DeliveryStatus::Pending),
            },
            generation,
        )
        .await?;
        self.require_neighbor_history_generation(generation)?;
        let result = if attachments.is_empty() {
            transport.send(&signed).await
        } else {
            files::send_envelope(transport, &signed, &attachments).await
        };
        let status = match &result {
            Ok(status) => *status,
            Err(Error::Transport) => codeterm_neighbors::wire::DeliveryStatus::Pending,
            Err(_) => codeterm_neighbors::wire::DeliveryStatus::NotDelivered,
        };
        if self.require_neighbor_history_generation(generation).is_ok() {
            let refusal = result
                .as_ref()
                .err()
                .copied()
                .and_then(super::super::delivery::Rejection::from_error);
            if let Err(error) = self
                .update_neighbor_history_status(
                    metadata,
                    &signed.envelope.message_id,
                    super::super::history::HistoryReceipt {
                        delivery: status.into(),
                        rejection: refusal,
                        submission: Some(match &result {
                            Ok(_) => super::super::history::Submission::Accepted,
                            Err(Error::Transport) => super::super::history::Submission::Unconfirmed,
                            Err(_) => super::super::history::Submission::Refused,
                        }),
                    },
                    generation,
                )
                .await
            {
                log::debug!("[neighbors] local delivery status write failed: {error}");
            }
        }
        let refusal = result
            .as_ref()
            .err()
            .copied()
            .and_then(super::super::delivery::Rejection::from_error);
        if let Some(rejection) = refusal {
            return Ok(super::super::delivery::SendResult {
                receipt: rejection.receipt(&signed.envelope.message_id),
                rejection: Some(rejection),
            });
        }
        let receipt = match result {
            Ok(status) => json!({"message_id":signed.envelope.message_id,"status":status}),
            Err(Error::Transport) => {
                json!({"message_id":signed.envelope.message_id,"status":"pending","submission":"unconfirmed"})
            }
            Err(error) => return Err(error),
        };
        Ok(super::super::delivery::SendResult {
            receipt,
            rejection: None,
        })
    }
}

const MAX_REFUSAL_BYTES: usize = 1024;

async fn prepare_outgoing_body(
    metadata: &Metadata,
    friendship: &str,
    permission: &Permission,
    kind: Kind,
    body: OutboundBody,
    in_reply_to: Option<&str>,
) -> codeterm_neighbors::Result<(Zeroizing<String>, Vec<String>)> {
    if matches!(kind, Kind::Answer | Kind::Refusal) {
        let history = metadata.history(friendship).await?;
        let target = in_reply_to.and_then(|id| {
            history
                .entries
                .iter()
                .find(|entry| entry.direction == "in" && entry.message_id == id)
        });
        if target.is_some_and(|entry| {
            entry.handling.handling_state == Some(super::super::history::handling::State::Stopped)
        }) {
            return Err(Error::ReplyTargetStopped);
        }
        if kind == Kind::Refusal && target.is_none() {
            return Err(Error::ReplyTargetUnknown);
        }
    }
    if kind == Kind::Refusal {
        let body = match body {
            OutboundBody::Granted(body) => body,
            OutboundBody::Refusal => {
                Zeroizing::new(codeterm_neighbors::gateway::REFUSAL_BODY.to_owned())
            }
            OutboundBody::Files { .. } => return Err(Error::InvalidRequest),
        };
        if body.len() > MAX_REFUSAL_BYTES {
            return Err(Error::RefusalTooLong {
                limit: MAX_REFUSAL_BYTES as u64,
                actual: body.len() as u64,
            });
        }
        return Ok((body, vec![]));
    }
    permission.require(kind)?;
    match body {
        OutboundBody::Granted(body) => Ok((body, vec![])),
        OutboundBody::Files { text, paths } => Ok((text, paths)),
        OutboundBody::Refusal => Err(Error::InvalidRequest),
    }
}

#[cfg(test)]
mod tests;
