mod local;
mod receive;
mod storage;
#[cfg(test)]
mod tests;
mod upload;
pub(super) use receive::prefetch;
use upload::upload;

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use codeterm_neighbors::files::{self, AttachmentBody, Binding, Manifest};
use md5::{Digest, Md5};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    Send {
        friendship_id: String,
        kind: Kind,
        body: String,
        in_reply_to: Option<String>,
        paths: Vec<String>,
    },
    List {
        friendship_id: String,
    },
    Open {
        friendship_id: String,
        file_id: String,
        launch: Option<bool>,
    },
    OpenFolder {
        friendship_id: String,
    },
    Usage {
        friendship_id: String,
    },
    SetDailyLimit {
        friendship_id: String,
        file_daily_limit_bytes: Option<serde_json::Number>,
    },
}

impl Command {
    fn friendship(&self) -> &str {
        match self {
            Self::Send { friendship_id, .. }
            | Self::List { friendship_id }
            | Self::Open { friendship_id, .. }
            | Self::OpenFolder { friendship_id }
            | Self::Usage { friendship_id }
            | Self::SetDailyLimit { friendship_id, .. } => friendship_id,
        }
    }
}

impl ServiceContext {
    pub async fn neighbor_files(
        &self,
        input: Value,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<Value, String> {
        let command: Command =
            serde_json::from_value(input).map_err(|_| Error::InvalidRequest.to_string())?;
        if !hex_id(command.friendship(), 64) {
            return Err(Error::InvalidRequest.to_string());
        }
        let mut concierge = None;
        if let Some(tab_id) = caller {
            if self.require_neighbor_general(caller, nonce).is_err() {
                if !matches!(&command, Command::List { .. } | Command::Open { .. }) {
                    return Err(
                        "neighbor_forbidden: This concierge cannot manage Neighbor files".into(),
                    );
                }
                let caller = crate::service::tab_credentials::TabCaller {
                    tab_id: tab_id.into(),
                    binding_nonce: nonce
                        .ok_or("neighbor_forbidden: Missing caller nonce")?
                        .into(),
                };
                self.authorize_neighbor_file_read(&caller, command.friendship())
                    .await?;
                concierge = Some(caller);
            }
        }
        if matches!(
            &command,
            Command::List { .. } | Command::Open { .. } | Command::OpenFolder { .. }
        ) {
            return self.neighbor_file_local(&command, concierge.as_ref()).await;
        }
        match self
            .neighbor_operation(owner::Operation::Files {
                command,
                caller: caller.map(str::to_owned),
                nonce: nonce.map(str::to_owned),
            })
            .await?
        {
            owner::Outcome::Value(value) => Ok(value),
        }
    }

    pub(super) async fn neighbor_files_owned(
        &self,
        command: Command,
        caller: Option<&str>,
        nonce: Option<&str>,
        generation: u64,
    ) -> Result<Value, String> {
        self.require_neighbor_generation(generation)?;
        if caller.is_some() {
            self.require_neighbor_general(caller, nonce)?;
        }
        let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
        self.require_neighbor_generation(generation)?;
        let friendship = command.friendship().to_owned();
        let result: codeterm_neighbors::Result<Value> = async {
            match command {
                Command::Send { kind, body, in_reply_to, paths, .. } => {
                    self.require_neighbors_enabled()?;
                    if paths.is_empty() || paths.len() > files::MAX_ATTACHMENTS || kind == Kind::Refusal {
                        return Err(Error::InvalidRequest);
                    }
                    codeterm_neighbors::wire::validate_message_size(&body)?;
                    let lifecycle = InstallationKeys { store: &OsKeyStore, registry: registry.as_ref(), scope, uid: &transport.uid };
                    let author = self.neighbor_author(caller)?;
                    let mut receipt = self.send_neighbor(transport.as_ref(), &lifecycle, &metadata, Outgoing {
                        authority: authority::SendAuthority::Unrestricted,
                        friendship: friendship.clone(), kind, body: OutboundBody::Files { text: Zeroizing::new(body), paths }, in_reply_to, author,
                    }).await?;
                    let entries = metadata.file_entries(&friendship).await?;
                    receipt["attachments"] = json!(entries.into_iter().filter(|entry| receipt["message_id"] == entry.message_id && entry.direction == "out").collect::<Vec<_>>());
                    Ok(receipt)
                }
                Command::List { .. } | Command::Open { .. } | Command::OpenFolder { .. } => Err(Error::InvalidRequest),
                Command::Usage { .. } => {
                    transport.calls.invoke("friendsFileUsage", json!({"installation_id":transport.installation_id,"friendship_id":friendship})).await
                }
                Command::SetDailyLimit { file_daily_limit_bytes, .. } => {
                    if file_daily_limit_bytes.as_ref().is_some_and(|number| number.as_u64().is_none_or(|bytes| bytes > 52_428_800)) {
                        return Err(Error::LimitOutOfRange);
                    }
                    let result = transport.calls.invoke("friendsSetFileDailyLimit", json!({"installation_id":transport.installation_id,"friendship_id":friendship,"file_daily_limit_bytes":file_daily_limit_bytes})).await?;
                    self.ui.emit("neighbor-file-daily-limit-changed", json!({"friendship_id":friendship,"file_daily_limit_bytes":result["file_daily_limit_bytes"]}));
                    Ok(result)
                }
            }
        }.await;
        self.require_neighbor_generation(generation)?;
        result.map_err(|error| error.to_string())
    }

    pub(super) async fn prepare_neighbor_files(
        &self,
        transport: &FirebaseTransport,
        metadata: &Metadata,
        envelope: &Envelope,
        text: Zeroizing<String>,
        paths: Vec<String>,
    ) -> codeterm_neighbors::Result<(Zeroizing<String>, String, Vec<String>)> {
        if paths.is_empty() {
            let projected = AttachmentBody::parse(&text, &binding(envelope))?;
            if projected.is_some() {
                return Err(Error::InvalidRequest);
            }
            return Ok((Zeroizing::new(text.to_string()), text.to_string(), vec![]));
        }
        if paths.len() > files::MAX_ATTACHMENTS {
            return Err(Error::InvalidRequest);
        }
        let generation = self.neighbor_history_generation();
        let envelope_copy = envelope.clone();
        let sealed = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut sealed = vec![];
            for path in paths {
                if !std::path::Path::new(&path).is_absolute() {
                    return Err(Error::InvalidRequest);
                }
                if !std::fs::metadata(&path)
                    .map_err(|_| Error::InvalidRequest)?
                    .is_file()
                {
                    return Err(Error::InvalidRequest);
                }
                let mut file = std::fs::File::open(&path).map_err(|_| Error::InvalidRequest)?;
                if !file
                    .metadata()
                    .map_err(|_| Error::InvalidRequest)?
                    .is_file()
                {
                    return Err(Error::InvalidRequest);
                }
                let mut bytes = Zeroizing::new(Vec::new());
                file.by_ref()
                    .take((files::MAX_FILE_BYTES - files::FILE_OVERHEAD + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| Error::InvalidRequest)?;
                if bytes.len() > files::MAX_FILE_BYTES - files::FILE_OVERHEAD {
                    return Err(Error::File(files::refusal::Failure::too_large()));
                }
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("attachment");
                sealed.push(files::seal(
                    &binding(&envelope_copy),
                    &uuid::Uuid::new_v4().simple().to_string(),
                    name,
                    "application/octet-stream",
                    &bytes,
                    &mut rand::rngs::OsRng,
                )?);
            }
            Ok::<_, Error>(sealed)
        })
        .await
        .map_err(|_| Error::Gateway)??;
        let manifests: Vec<_> = sealed
            .iter()
            .map(|(manifest, _)| manifest.clone())
            .collect();
        let typed = AttachmentBody::new(text.to_string(), manifests);
        let encoded = typed.encode(&binding(envelope))?;
        let visible = typed.agent_text();
        let ids: Vec<_> = typed
            .attachments
            .iter()
            .map(|file| file.file_id.clone())
            .collect();
        for (manifest, object) in sealed {
            self.require_neighbor_history_generation(generation)?;
            upload(self, transport, envelope, &manifest, object).await?;
            self.require_neighbor_history_generation(generation)?;
            self.record_neighbor_file(
                metadata,
                &envelope.friendship_id,
                storage::Entry::new(&manifest, envelope, "out", "saved"),
            )
            .await?;
        }
        Ok((encoded, visible, ids))
    }
}

pub(super) fn binding(envelope: &Envelope) -> Binding<'_> {
    Binding {
        friendship_id: &envelope.friendship_id,
        sender_uid: &envelope.sender_uid,
        recipient_uid: &envelope.recipient_uid,
        message_id: &envelope.message_id,
    }
}

pub(super) async fn send_envelope(
    transport: &FirebaseTransport,
    signed: &codeterm_neighbors::wire::SignedEnvelope,
    file_ids: &[String],
) -> codeterm_neighbors::Result<codeterm_neighbors::wire::DeliveryStatus> {
    signed.envelope.validate()?;
    if signed.envelope.sender_uid != transport.uid {
        return Err(Error::WrongPeer);
    }
    let mut data = serde_json::to_value(signed).map_err(|_| Error::InvalidEnvelope)?;
    data["installation_id"] = json!(transport.installation_id);
    data["file_ids"] = json!(file_ids);
    if let Some(reply) = &signed.envelope.in_reply_to {
        data["in_reply_to"] = json!(reply);
    }
    transport.calls.invoke("friendsSend", data).await?;
    Ok(codeterm_neighbors::wire::DeliveryStatus::Pending)
}
