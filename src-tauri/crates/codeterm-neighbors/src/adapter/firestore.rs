use super::FirebaseTransport;
use crate::receive::InboxListener;
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

pub(super) async fn listen(
    transport: &FirebaseTransport,
    friendship: &str,
    cursor: Option<&Cursor>,
) -> Result<Box<dyn InboxListener>> {
    if !hex_id(friendship, 64)
        || !crate::wire::uid(&transport.uid)
        || transport.project.is_empty()
        || !transport
            .project
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::InvalidEnvelope);
    }
    if let Some(cursor) = cursor {
        cursor.validate()?;
    }
    let channel = Channel::from_static("https://firestore.googleapis.com")
        .tls_config(ClientTlsConfig::new().with_native_roots())
        .map_err(|_| Error::Transport)?
        .connect_timeout(Duration::from_secs(15))
        .connect()
        .await
        .map_err(|_| Error::Transport)?;
    let token = transport.calls.id_token().await?;
    let database = format!("projects/{}/databases/(default)", transport.project);
    let parent = format!("{database}/documents/friendInbox/{}", transport.uid);
    Ok(Box::new(Listener {
        client: FirestoreClient::new(channel).max_decoding_message_size(2 * 1024 * 1024),
        token,
        database,
        parent,
        friendship: friendship.into(),
        cursor: cursor.cloned(),
        catch_up: true,
        stream: None,
        requests: None,
        buffered: HashMap::new(),
    }))
}

struct Listener {
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
}

impl Listener {
    fn request<T>(&self, message: T) -> Result<tonic::Request<T>> {
        let mut request = tonic::Request::new(message);
        let header = Zeroizing::new(format!("Bearer {}", self.token.as_str()));
        let mut metadata = header
            .parse::<tonic::metadata::MetadataValue<_>>()
            .map_err(|_| Error::Transport)?;
        metadata.set_sensitive(true);
        request.metadata_mut().insert("authorization", metadata);
        request.metadata_mut().insert(
            "google-cloud-resource-prefix",
            self.database.parse().map_err(|_| Error::Transport)?,
        );
        Ok(request)
    }

    fn query(&self, limit: Option<i32>) -> fs::StructuredQuery {
        use fs::structured_query::{self as q, field_filter::Operator, filter::FilterType};
        fs::StructuredQuery {
            from: vec![q::CollectionSelector {
                collection_id: "messages".into(),
                all_descendants: false,
            }],
            r#where: Some(q::Filter {
                filter_type: Some(FilterType::FieldFilter(q::FieldFilter {
                    field: Some(q::FieldReference {
                        field_path: "envelope.friendship_id".into(),
                    }),
                    op: Operator::Equal as i32,
                    value: Some(value(fs::value::ValueType::StringValue(
                        self.friendship.clone(),
                    ))),
                })),
            }),
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
    async fn next(&mut self) -> Result<Option<Vec<InboxMessage>>> {
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
                .map_err(|_| Error::Transport)?
                .into_inner();
            let mut messages = Vec::new();
            while let Some(response) = responses.message().await.map_err(|_| Error::Transport)? {
                if let Some(doc) = response.document {
                    messages.push(parse_document(doc, &self.parent)?);
                }
                if messages.len() > PAGE_SIZE as usize {
                    return Err(Error::Transport);
                }
            }
            self.catch_up = messages.len() == PAGE_SIZE as usize;
            if !messages.is_empty() {
                return Ok(Some(self.sorted(messages)));
            }
        }
        if self.stream.is_none() {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            sender
                .send(fs::ListenRequest {
                    database: self.database.clone(),
                    target_change: Some(fs::listen_request::TargetChange::AddTarget(fs::Target {
                        target_type: Some(fs::target::TargetType::Query(fs::target::QueryTarget {
                            parent: self.parent.clone(),
                            query_type: Some(fs::target::query_target::QueryType::StructuredQuery(
                                self.query(None),
                            )),
                        })),
                        target_id: 1,
                        ..Default::default()
                    })),
                    ..Default::default()
                })
                .await
                .map_err(|_| Error::Transport)?;
            let request = self.request(tokio_stream::wrappers::ReceiverStream::new(receiver))?;
            self.stream = Some(
                self.client
                    .listen(request)
                    .await
                    .map_err(|_| Error::Transport)?
                    .into_inner(),
            );
            self.requests = Some(sender);
        }
        loop {
            let response = self
                .stream
                .as_mut()
                .ok_or(Error::Transport)?
                .message()
                .await
                .map_err(|_| Error::Transport)?;
            let Some(response) = response else {
                return Ok(None);
            };
            use fs::listen_response::ResponseType;
            match response.response_type {
                Some(ResponseType::DocumentChange(change)) => {
                    let message =
                        parse_document(change.document.ok_or(Error::Transport)?, &self.parent)?;
                    if self.buffered.len() >= 500 {
                        return Err(Error::Transport);
                    }
                    self.buffered.insert(message.id.clone(), message);
                }
                Some(ResponseType::DocumentDelete(change)) => {
                    self.buffered
                        .remove(change.document.rsplit('/').next().ok_or(Error::Transport)?);
                }
                Some(ResponseType::DocumentRemove(change)) => {
                    self.buffered
                        .remove(change.document.rsplit('/').next().ok_or(Error::Transport)?);
                }
                Some(ResponseType::TargetChange(change)) => {
                    use fs::target_change::TargetChangeType;
                    match TargetChangeType::try_from(change.target_change_type)
                        .map_err(|_| Error::Transport)?
                    {
                        TargetChangeType::Remove | TargetChangeType::Reset => {
                            return Err(Error::Transport)
                        }
                        TargetChangeType::NoChange | TargetChangeType::Current
                            if change.read_time.is_some() =>
                        {
                            let messages =
                                self.buffered.drain().map(|(_, message)| message).collect();
                            let messages = self.sorted(messages);
                            if !messages.is_empty() {
                                return Ok(Some(messages));
                            }
                        }
                        _ => {}
                    }
                }
                Some(ResponseType::Filter(_)) => return Err(Error::Transport),
                None => return Err(Error::Transport),
            }
        }
    }
}

fn value(value_type: fs::value::ValueType) -> fs::Value {
    fs::Value {
        value_type: Some(value_type),
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
