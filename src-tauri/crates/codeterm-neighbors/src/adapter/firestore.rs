use super::AuthenticatedCall;
pub(super) mod control;
use super::FirebaseTransport;
use crate::receive::{InboxEvent, InboxListener};
use crate::wire::{hex_id, Cursor, InboxDocument, InboxMessage, Timestamp};
use crate::{Error, Result};
use async_trait::async_trait;
use googleapis_tonic_google_firestore_v1::google::firestore::v1::{
    self as fs, firestore_client::FirestoreClient,
};
use std::{collections::HashMap, time::Duration};
use tonic::transport::{Channel, ClientTlsConfig};
use zeroize::Zeroizing;

const PAGE_SIZE: i32 = 100;
mod pending;
pub(super) mod registry;
pub(super) use pending::count as pending_count;
mod watch;

async fn connection(
    transport: &FirebaseTransport,
) -> Result<(FirestoreClient<Channel>, Zeroizing<String>, String)> {
    if !crate::wire::uid(&transport.uid)
        || transport.project.is_empty()
        || !transport
            .project
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::InvalidEnvelope);
    }
    let channel = Channel::from_static("https://firestore.googleapis.com")
        .tls_config(ClientTlsConfig::new().with_native_roots())
        .map_err(|_| Error::Transport)?
        .connect_timeout(Duration::from_secs(15))
        .connect()
        .await
        .map_err(|_| {
            transport.calls.firestore_failure(
                "connect",
                "firestore.googleapis.com",
                None,
                "transport_connect",
            );
            Error::Transport
        })?;
    Ok((
        FirestoreClient::new(channel).max_decoding_message_size(2 * 1024 * 1024),
        transport.calls.id_token().await?,
        format!("projects/{}/databases/(default)", transport.project),
    ))
}

fn rpc_failure(
    calls: &dyn AuthenticatedCall,
    operation: &'static str,
    target: &str,
    status: tonic::Status,
) -> Error {
    use std::error::Error as _;
    if let Some(source) = status.source() {
        let mut source = Some(source);
        let mut kind = "source_other";
        while let Some(error) = source {
            if let Some(error) = error.downcast_ref::<std::io::Error>() {
                let kind = format!("io_{:?}", error.kind());
                calls.firestore_failure(operation, target, Some(status.code() as i32), &kind);
                break;
            }
            let text = error.to_string().to_ascii_lowercase();
            if text.contains("http2") || text.contains("http/2") || text.contains("h2") {
                kind = "http2";
            } else if text.contains("decode") || text.contains("protobuf") {
                kind = "response_decode";
            }
            source = error.source();
        }
        calls.firestore_failure(operation, target, Some(status.code() as i32), kind);
    }
    code_failure(calls, operation, target, status.code() as i32)
}

fn code_failure(
    calls: &dyn AuthenticatedCall,
    operation: &'static str,
    target: &str,
    code: i32,
) -> Error {
    let kind = format!("{:?}", tonic::Code::from_i32(code));
    calls.firestore_failure(operation, target, Some(code), &kind);
    Error::Firestore { code, operation }
}

fn authenticated_request<T>(token: &str, database: &str, message: T) -> Result<tonic::Request<T>> {
    let mut request = tonic::Request::new(message);
    let header = Zeroizing::new(format!("Bearer {token}"));
    let mut metadata = header
        .parse::<tonic::metadata::MetadataValue<_>>()
        .map_err(|_| Error::Transport)?;
    metadata.set_sensitive(true);
    request.metadata_mut().insert("authorization", metadata);
    request.metadata_mut().insert(
        "google-cloud-resource-prefix",
        database.parse().map_err(|_| Error::Transport)?,
    );
    request.metadata_mut().insert(
        "x-goog-request-params",
        format!("database={database}")
            .parse()
            .map_err(|_| Error::Transport)?,
    );
    Ok(request)
}

pub(super) async fn listen(
    transport: &FirebaseTransport,
    friendship: &str,
    cursor: Option<&Cursor>,
) -> Result<Box<dyn InboxListener>> {
    listen_mode(transport, friendship, cursor, false).await
}

pub(super) async fn listen_pending(
    transport: &FirebaseTransport,
    friendship: &str,
) -> Result<Box<dyn InboxListener>> {
    listen_mode(transport, friendship, None, true).await
}

async fn listen_mode(
    transport: &FirebaseTransport,
    friendship: &str,
    cursor: Option<&Cursor>,
    observe_only: bool,
) -> Result<Box<dyn InboxListener>> {
    if !hex_id(friendship, 64) {
        return Err(Error::InvalidEnvelope);
    }
    if let Some(cursor) = cursor {
        cursor.validate()?;
    }
    let (client, token, database) = connection(transport).await?;
    let parent = format!("{database}/documents/friendInbox/{}", transport.uid);
    Ok(Box::new(Listener {
        client,
        token,
        database,
        parent,
        friendship: friendship.into(),
        cursor: cursor.cloned(),
        catch_up: !observe_only,
        observe_only,
        dirty: false,
        stream: None,
        requests: None,
        buffered: HashMap::new(),
        pending: None,
        membership: watch::Membership::new(observe_only),
        resetting: false,
        reset_budget: Default::default(),
        calls: transport.calls.clone(),
    }))
}

struct Listener {
    observe_only: bool,
    dirty: bool,
    resetting: bool,
    reset_budget: watch::ResetBudget,
    membership: watch::Membership,
    calls: std::sync::Arc<dyn AuthenticatedCall>,
    client: FirestoreClient<Channel>,
    token: Zeroizing<String>,
    database: String,
    parent: String,
    friendship: String,
    cursor: Option<Cursor>,
    catch_up: bool,
    stream: Option<tonic::Streaming<fs::ListenResponse>>,
    requests: Option<tokio::sync::mpsc::Sender<fs::ListenRequest>>,
    buffered: HashMap<String, InboxMessage>,
    pending: Option<Vec<InboxMessage>>,
}

impl Listener {
    fn target(&self) -> fs::Target {
        fs::Target {
            target_type: Some(fs::target::TargetType::Query(fs::target::QueryTarget {
                parent: self.parent.clone(),
                query_type: Some(fs::target::query_target::QueryType::StructuredQuery(
                    self.query(
                        self.observe_only
                            .then_some(super::PENDING_COUNT_LIMIT as i32),
                    ),
                )),
            })),
            target_id: 1,
            ..Default::default()
        }
    }

    async fn reset_target(&mut self) -> Result<()> {
        self.reset_budget.consume()?;
        let sender = self.requests.as_ref().ok_or(Error::ListenProtocol {
            kind: "request_stream_missing",
        })?;
        for target_change in [
            fs::listen_request::TargetChange::RemoveTarget(1),
            fs::listen_request::TargetChange::AddTarget(self.target()),
        ] {
            sender
                .send(fs::ListenRequest {
                    database: self.database.clone(),
                    target_change: Some(target_change),
                    ..Default::default()
                })
                .await
                .map_err(|_| Error::ListenProtocol {
                    kind: "request_stream_closed",
                })?;
        }
        self.membership = watch::Membership::new(self.observe_only);
        self.buffered.clear();
        self.resetting = true;
        Ok(())
    }

    fn request<T>(&self, message: T) -> Result<tonic::Request<T>> {
        authenticated_request(&self.token, &self.database, message)
    }

    fn query(&self, limit: Option<i32>) -> fs::StructuredQuery {
        use fs::structured_query as q;
        fs::StructuredQuery {
            from: vec![q::CollectionSelector {
                collection_id: "messages".into(),
                all_descendants: false,
            }],
            r#where: Some(friendship_filter(&self.friendship)),
            order_by: ["received_at", "__name__"]
                .into_iter()
                .map(|field| q::Order {
                    field: Some(q::FieldReference {
                        field_path: field.into(),
                    }),
                    direction: q::Direction::Ascending as i32,
                })
                .collect(),
            start_at: self.cursor.as_ref().map(|cursor| fs::Cursor {
                values: vec![
                    value(fs::value::ValueType::TimestampValue(
                        prost_types::Timestamp {
                            seconds: cursor.received_at.seconds as i64,
                            nanos: cursor.received_at.nanoseconds as i32,
                        },
                    )),
                    value(fs::value::ValueType::ReferenceValue(format!(
                        "{}/messages/{}",
                        self.parent, cursor.id
                    ))),
                ],
                before: false,
            }),
            limit,
            ..Default::default()
        }
    }

    fn sorted(&mut self, mut messages: Vec<InboxMessage>) -> Vec<InboxMessage> {
        messages.sort_by(|a, b| {
            a.doc
                .received_at
                .cmp(&b.doc.received_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        messages.retain(|message| {
            self.cursor.as_ref().is_none_or(|previous| {
                Cursor {
                    received_at: message.doc.received_at,
                    id: message.id.clone(),
                } > *previous
            })
        });
        if let Some(last) = messages.last() {
            self.cursor = Some(Cursor {
                received_at: last.doc.received_at,
                id: last.id.clone(),
            });
        }
        messages
    }
}

#[async_trait]
impl InboxListener for Listener {
    async fn next(&mut self) -> Result<Option<InboxEvent>> {
        if let Some(messages) = self.pending.take() {
            return Ok(Some(InboxEvent::Messages(messages)));
        }
        if self.catch_up {
            let request = self.request(fs::RunQueryRequest {
                parent: self.parent.clone(),
                query_type: Some(fs::run_query_request::QueryType::StructuredQuery(
                    self.query(Some(PAGE_SIZE)),
                )),
                ..Default::default()
            })?;
            let mut responses = self
                .client
                .run_query(request)
                .await
                .map_err(|status| {
                    rpc_failure(self.calls.as_ref(), "inbox.run_query", &self.parent, status)
                })?
                .into_inner();
            let mut messages = Vec::new();
            while let Some(response) = responses.message().await.map_err(|status| {
                rpc_failure(
                    self.calls.as_ref(),
                    "inbox.run_query.read",
                    &self.parent,
                    status,
                )
            })? {
                if let Some(doc) = response.document {
                    messages.push(parse_document(doc, &self.parent)?);
                }
                if messages.len() > PAGE_SIZE as usize {
                    return Err(Error::Transport);
                }
            }
            self.catch_up = messages.len() == PAGE_SIZE as usize;
            if !messages.is_empty() {
                return Ok(Some(InboxEvent::Messages(self.sorted(messages))));
            }
        }
        if self.stream.is_none() {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            sender
                .send(fs::ListenRequest {
                    database: self.database.clone(),
                    target_change: Some(fs::listen_request::TargetChange::AddTarget(self.target())),
                    ..Default::default()
                })
                .await
                .map_err(|_| Error::Transport)?;
            let request = self.request(tokio_stream::wrappers::ReceiverStream::new(receiver))?;
            self.stream = Some(
                self.client
                    .listen(request)
                    .await
                    .map_err(|status| {
                        rpc_failure(self.calls.as_ref(), "inbox.listen", &self.parent, status)
                    })?
                    .into_inner(),
            );
            self.requests = Some(sender);
        }
        loop {
            let response = self
                .stream
                .as_mut()
                .ok_or(Error::ListenProtocol {
                    kind: "response_stream_missing",
                })?
                .message()
                .await
                .map_err(|status| {
                    rpc_failure(
                        self.calls.as_ref(),
                        "inbox.listen.read",
                        &self.parent,
                        status,
                    )
                })?;
            let Some(response) = response else {
                return Err(Error::ListenProtocol {
                    kind: "stream_closed",
                });
            };
            if self.resetting
                && matches!(&response.response_type, Some(fs::listen_response::ResponseType::TargetChange(change)) if change.cause.is_none() && change.target_change_type == fs::target_change::TargetChangeType::Remove as i32 && change.target_ids.contains(&1))
            {
                self.resetting = false;
                continue;
            }
            match self.membership.observe(&response) {
                Err(Error::ListenProtocol {
                    kind: "existence_filter_mismatch",
                }) => {
                    self.calls.firestore_failure(
                        "inbox.listen.reset",
                        &self.parent,
                        None,
                        "existence_filter_mismatch",
                    );
                    self.reset_target().await?;
                    continue;
                }
                Ok(true) => {
                    self.buffered.clear();
                    self.dirty = true;
                }
                result => {
                    result?;
                }
            }
            use fs::listen_response::ResponseType;
            match response.response_type {
                Some(ResponseType::DocumentChange(change)) => {
                    if self.observe_only {
                        self.dirty = true;
                        continue;
                    }
                    let message = parse_document(
                        change.document.ok_or(Error::ListenProtocol {
                            kind: "document_missing",
                        })?,
                        &self.parent,
                    )?;
                    if self.buffered.len() >= 500 {
                        return Err(Error::ListenProtocol {
                            kind: "document_buffer_limit",
                        });
                    }
                    self.buffered.insert(message.id.clone(), message);
                }
                Some(ResponseType::DocumentDelete(change)) => {
                    if self.observe_only {
                        self.dirty = true;
                        continue;
                    }
                    self.buffered
                        .remove(change.document.rsplit('/').next().ok_or(Error::Transport)?);
                }
                Some(ResponseType::DocumentRemove(change)) => {
                    if self.observe_only {
                        self.dirty = true;
                        continue;
                    }
                    self.buffered
                        .remove(change.document.rsplit('/').next().ok_or(Error::Transport)?);
                }
                Some(ResponseType::TargetChange(change)) => {
                    if let Some(cause) = change.cause.as_ref() {
                        return Err(code_failure(
                            self.calls.as_ref(),
                            "inbox.listen.target",
                            &self.parent,
                            cause.code,
                        ));
                    }
                    let checkpoint = target_checkpoint(&change)?;
                    if self.observe_only {
                        if checkpoint.ready || checkpoint.flush && self.dirty {
                            self.dirty = false;
                            return Ok(Some(InboxEvent::Ready));
                        }
                        continue;
                    }
                    let messages = if checkpoint.flush {
                        let messages = self.buffered.drain().map(|(_, message)| message).collect();
                        self.sorted(messages)
                    } else {
                        Vec::new()
                    };
                    if checkpoint.ready {
                        if !messages.is_empty() {
                            self.pending = Some(messages);
                        }
                        return Ok(Some(InboxEvent::Ready));
                    }
                    if !messages.is_empty() {
                        return Ok(Some(InboxEvent::Messages(messages)));
                    }
                }
                Some(ResponseType::Filter(_)) => {
                    if self.observe_only {
                        self.dirty = true;
                    }
                }
                None => {
                    return Err(Error::ListenProtocol {
                        kind: "response_missing",
                    })
                }
            }
        }
    }
}

struct TargetCheckpoint {
    ready: bool,
    flush: bool,
}

fn target_checkpoint(change: &fs::TargetChange) -> Result<TargetCheckpoint> {
    use fs::target_change::TargetChangeType;
    if change.cause.is_some() {
        return Err(Error::Transport);
    }
    let kind = TargetChangeType::try_from(change.target_change_type).map_err(|_| {
        Error::ListenProtocol {
            kind: "target_change_unknown",
        }
    })?;
    if kind == TargetChangeType::Remove {
        return Err(Error::ListenProtocol {
            kind: "target_removed",
        });
    }
    let own_target = change.target_ids.is_empty() || change.target_ids.contains(&1);
    Ok(TargetCheckpoint {
        ready: own_target && kind == TargetChangeType::Current,
        flush: own_target
            && change.read_time.is_some()
            && matches!(kind, TargetChangeType::Current | TargetChangeType::NoChange),
    })
}

#[cfg(test)]
mod tests;

fn value(value_type: fs::value::ValueType) -> fs::Value {
    fs::Value {
        value_type: Some(value_type),
    }
}

fn friendship_filter(friendship: &str) -> fs::structured_query::Filter {
    use fs::structured_query::{self as q, field_filter::Operator, filter::FilterType};
    q::Filter {
        filter_type: Some(FilterType::FieldFilter(q::FieldFilter {
            field: Some(q::FieldReference {
                field_path: "envelope.friendship_id".into(),
            }),
            op: Operator::Equal as i32,
            value: Some(value(fs::value::ValueType::StringValue(friendship.into()))),
        })),
    }
}

fn timestamp(value: fs::Value) -> Result<Timestamp> {
    let Some(fs::value::ValueType::TimestampValue(time)) = value.value_type else {
        return Err(Error::InvalidEnvelope);
    };
    let time = Timestamp {
        seconds: time
            .seconds
            .try_into()
            .map_err(|_| Error::InvalidEnvelope)?,
        nanoseconds: time.nanos.try_into().map_err(|_| Error::InvalidEnvelope)?,
    };
    if !time.valid() {
        return Err(Error::InvalidEnvelope);
    }
    Ok(time)
}

fn json(value: fs::Value, depth: usize) -> Result<serde_json::Value> {
    if depth > 3 {
        return Err(Error::InvalidEnvelope);
    }
    use fs::value::ValueType;
    match value.value_type {
        Some(ValueType::StringValue(s)) if s.len() <= 12_000 => Ok(s.into()),
        Some(ValueType::IntegerValue(i)) if i >= 0 && i as u64 <= crate::wire::MAX_SAFE_INTEGER => {
            Ok(i.into())
        }
        Some(ValueType::NullValue(_)) => Ok(serde_json::Value::Null),
        Some(ValueType::MapValue(map)) if map.fields.len() <= 20 => {
            let map = map
                .fields
                .into_iter()
                .map(|(k, v)| Ok((k, json(v, depth + 1)?)))
                .collect::<Result<serde_json::Map<_, _>>>()?;
            Ok(serde_json::Value::Object(map))
        }
        _ => Err(Error::InvalidEnvelope),
    }
}

fn parse_document(document: fs::Document, parent: &str) -> Result<InboxMessage> {
    let id = document
        .name
        .strip_prefix(&format!("{parent}/messages/"))
        .ok_or(Error::WrongPeer)?
        .to_owned();
    let mut fields = document.fields;
    if fields.len() != 4 {
        return Err(Error::InvalidEnvelope);
    }
    let envelope = json(fields.remove("envelope").ok_or(Error::InvalidEnvelope)?, 0)?;
    let signature = json(fields.remove("signature").ok_or(Error::InvalidEnvelope)?, 0)?;
    let received_at = timestamp(fields.remove("received_at").ok_or(Error::InvalidEnvelope)?)?;
    let expires_at = timestamp(fields.remove("expiresAt").ok_or(Error::InvalidEnvelope)?)?;
    Cursor {
        received_at,
        id: id.clone(),
    }
    .validate()?;
    Ok(InboxMessage {
        id,
        doc: InboxDocument {
            envelope: serde_json::from_value(envelope).map_err(|_| Error::InvalidEnvelope)?,
            signature: signature.as_str().ok_or(Error::InvalidEnvelope)?.to_owned(),
            received_at,
            expires_at,
        },
    })
}
