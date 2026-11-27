use super::enrolled_wire::ChannelIo;
use super::{EnrolledChannel, IngressError};
use crate::account_mesh::BoxFuture;
use crate::cloud_outbox::{
    CloudOutbox, CloudOutboxPort, DedupeDecision, OutboxFrame, OutboxId, OuterWireRequest,
    ReceiverDedupe, ReverseStreamSink, StreamDeliveryOutcome, UnconfirmedReason,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

pub trait ReverseDispatcher: Send + Sync {
    fn enabled(&self) -> bool;
    fn dispatch(
        &self,
        request: OuterWireRequest,
        signed_token: String,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, IngressError>>;
}

use crate::cloud_outbox::{
    DrainAuthorizationPolicy, DrainPolicyDecision, OutboxClock, SystemOutboxClock,
};

pub(super) struct InstalledPolicy {
    pub caller: std::sync::OnceLock<Arc<dyn DrainAuthorizationPolicy>>,
    pub dispatcher: Arc<dyn ReverseDispatcher>,
}
impl DrainAuthorizationPolicy for InstalledPolicy {
    fn authorize_drain(&self, frame: &OutboxFrame, now_ms: u64) -> DrainPolicyDecision {
        if !self.dispatcher.enabled() {
            return DrainPolicyDecision::Defer {
                reason: "mesh_disabled".into(),
            };
        }
        match self.caller.get() {
            Some(policy) => policy.authorize_drain(frame, now_ms),
            None => DrainPolicyDecision::Defer {
                reason: "caller_authority_unavailable".into(),
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    frame: OutboxFrame,
    signed_token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    id: OutboxId,
    digest: String,
    outcome: StreamDeliveryOutcome,
}

fn uncertain(reason: UnconfirmedReason, written: Option<bool>) -> StreamDeliveryOutcome {
    StreamDeliveryOutcome::Unconfirmed {
        reason,
        bytes_written: written,
        detail: None,
    }
}

struct Pending {
    frame: OutboxFrame,
    reply: oneshot::Sender<StreamDeliveryOutcome>,
}

struct Sink {
    stream_id: u64,
    account: String,
    local: String,
    remote: String,
    commands: mpsc::Sender<Pending>,
}

impl ReverseStreamSink for Sink {
    fn stream_id(&self) -> u64 {
        self.stream_id
    }
    fn account_id(&self) -> &str {
        &self.account
    }
    fn local_installation_id(&self) -> &str {
        &self.local
    }
    fn peer_installation_id(&self) -> &str {
        &self.remote
    }
    fn deliver<'a>(&'a self, frame: &'a OutboxFrame) -> BoxFuture<'a, StreamDeliveryOutcome> {
        Box::pin(async move {
            let (reply, receipt) = oneshot::channel();
            if self
                .commands
                .send(Pending {
                    frame: frame.clone(),
                    reply,
                })
                .await
                .is_err()
            {
                return StreamDeliveryOutcome::NotWritten {
                    reason: "stream_detached".into(),
                };
            }
            receipt
                .await
                .unwrap_or_else(|_| uncertain(UnconfirmedReason::StreamClosedBeforeAck, None))
        })
    }
}

struct Attached(Arc<CloudOutbox>, u64);
impl Drop for Attached {
    fn drop(&mut self) {
        self.0.detach_reverse_stream(self.1);
    }
}

fn validate<S: ChannelIo>(
    channel: &EnrolledChannel<S>,
    frame: &OutboxFrame,
    sending: bool,
) -> Result<(), IngressError> {
    let manager = channel.mesh.lock();
    channel
        .peer
        .peer_for_dispatch(&manager, &channel.stream.binding)?;
    let peer = channel.peer.remote_membership();
    let local = channel.peer.local_id();
    let remote = peer.installation_id.as_str();
    let (source, target) = if sending {
        (local, remote)
    } else {
        (remote, local)
    };
    let req = &frame.request;
    if req.source_installation_id != source
        || req.target_installation_id != target
        || req.account_id != peer.account_id
        || req.caller_context.expires_at_ms.is_some_and(|end| {
            crate::cloud_outbox::OutboxClock::now_ms(&crate::cloud_outbox::SystemOutboxClock) >= end
        })
    {
        return Err(IngressError::Rejected);
    }
    drop(manager);
    req.validate().map_err(|_| IngressError::InvalidFrame)?;
    if req.payload_digest() != frame.payload_digest {
        return Err(IngressError::Rejected);
    }
    Ok(())
}

async fn deliver<S: ChannelIo>(
    channel: &mut EnrolledChannel<S>,
    frame: OutboxFrame,
) -> StreamDeliveryOutcome {
    let token = (|| {
        validate(channel, &frame, true)?;
        let manager = channel.mesh.lock();
        let peer = channel
            .peer
            .peer_for_dispatch(&manager, &channel.stream.binding)?;
        if !manager.peer_requests_allowed(&peer.instance_id) {
            return Err(IngressError::Rejected);
        }
        let local = manager.admission_context()?;
        let caller = &frame.request.caller_context;
        crate::crown::sign_request_with_agent(
            &local.private_key,
            &local.party.instance_id,
            caller.origin,
            caller.agent.as_ref(),
        )
        .map_err(|_| IngressError::Rejected)
    })();
    let Ok(signed_token) = token else {
        return StreamDeliveryOutcome::NotWritten {
            reason: "admission_refused".into(),
        };
    };
    let id = frame.id.clone();
    let digest = frame.payload_digest.clone();
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(30),
            channel.stream.write(&Delivery {
                frame,
                signed_token
            })
        )
        .await,
        Ok(Ok(()))
    ) {
        return uncertain(UnconfirmedReason::WriteInterrupted, None);
    }
    match tokio::time::timeout(Duration::from_secs(60), channel.stream.read::<Ack>()).await {
        Ok(Ok(Ack {
            id: reply_id,
            digest: reply_digest,
            outcome: acked @ StreamDeliveryOutcome::Acked { .. },
        })) if reply_id == id && reply_digest == digest && channel.admit().is_ok() => acked,
        Err(_) => uncertain(UnconfirmedReason::AckTimeout, Some(true)),
        _ => uncertain(UnconfirmedReason::StreamClosedBeforeAck, Some(true)),
    }
}

pub(crate) async fn serve<S: ChannelIo>(
    mut channel: EnrolledChannel<S>,
    outbox: Arc<CloudOutbox>,
    policy: Arc<dyn DrainAuthorizationPolicy>,
) -> Result<(), IngressError> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let stream_id = NEXT.fetch_add(1, Ordering::Relaxed);
    let remote = channel.peer.remote_membership();
    let (commands, mut queue) = mpsc::channel(1);
    let sink = Arc::new(Sink {
        stream_id,
        account: remote.account_id.clone(),
        local: channel.peer.local_id().into(),
        remote: remote.installation_id.clone(),
        commands,
    });
    let peer_id = channel
        .peer
        .peer_for_dispatch(&channel.mesh.lock(), &channel.stream.binding)?
        .instance_id
        .clone();
    let blocked = crate::account_mesh::wait_until_blocked(channel.mesh.clone(), peer_id);
    tokio::pin!(blocked);
    outbox.attach_reverse_stream(sink).await;
    let _attached = Attached(outbox, stream_id);
    channel.admit()?;
    channel.stream.write(&()).await?;
    loop {
        let command = tokio::select! {
            _ = &mut blocked => return Err(IngressError::Rejected),
            _ = channel.stream.read::<()>() => return Err(IngressError::InvalidFrame),
            item=queue.recv()=>item.ok_or(IngressError::Unavailable)?,
        };
        let outcome = match policy.authorize_drain(&command.frame, SystemOutboxClock.now_ms()) {
            DrainPolicyDecision::Allow => deliver(&mut channel, command.frame).await,
            DrainPolicyDecision::Defer { reason } | DrainPolicyDecision::Refuse { reason } => {
                StreamDeliveryOutcome::NotWritten { reason }
            }
        };
        let keep = matches!(outcome, StreamDeliveryOutcome::Acked { .. });
        let _ = command.reply.send(outcome);
        if !keep {
            return Err(IngressError::Unavailable);
        }
    }
}

#[cfg(test)]
pub(crate) async fn receive<S: ChannelIo>(
    channel: EnrolledChannel<S>,
    dedupe: &ReceiverDedupe,
    dispatcher: &dyn ReverseDispatcher,
) -> Result<(), IngressError> {
    receive_with_attach(channel, dedupe, dispatcher, || {}).await
}

pub(super) async fn receive_with_attach<S: ChannelIo>(
    mut channel: EnrolledChannel<S>,
    dedupe: &ReceiverDedupe,
    dispatcher: &dyn ReverseDispatcher,
    attached: impl FnOnce() + Send,
) -> Result<(), IngressError> {
    channel.stream.read::<()>().await?;
    channel.admit()?;
    let id = channel.peer.remote_membership().installation_id.clone();
    let binding = channel.stream.binding;
    let _presence = StreamPresence(channel.mesh.clone(), id.clone(), binding);
    channel.mesh.lock().attach_reverse_stream(id, binding);
    attached();
    loop {
        let Delivery {
            frame,
            signed_token,
        } = channel.stream.read().await?;
        if !dispatcher.enabled() {
            return Err(IngressError::Rejected);
        }
        validate(&channel, &frame, false)?;
        let remote = channel
            .peer
            .peer_for_dispatch(&channel.mesh.lock(), &channel.stream.binding)?;
        let keys = [(remote.instance_id.clone(), remote.mesh_public_key.clone())].into();
        let claims = crate::crown::verify_request_claims(&signed_token, &keys)
            .map_err(|_| IngressError::Rejected)?;
        let caller = &frame.request.caller_context;
        if claims.origin != caller.origin || claims.agent != caller.agent {
            return Err(IngressError::Rejected);
        }
        let key = frame.dedupe_key();
        let reply_body = match dedupe
            .reserve(key.clone(), frame.payload_digest.clone())
            .await
            .map_err(|_| IngressError::Rejected)?
        {
            DedupeDecision::Execute => {
                let request = frame
                    .request
                    .to_outer_wire_request()
                    .map_err(|_| IngressError::InvalidFrame)?;
                validate(&channel, &frame, false)?;
                let reply = dispatcher.dispatch(request, signed_token).await?;
                dedupe
                    .complete(&key, reply.clone())
                    .await
                    .map_err(|_| IngressError::Unavailable)?;
                reply
            }
            DedupeDecision::DuplicateCompleted { reply_body } => reply_body,
            _ => return Err(IngressError::Unavailable),
        };
        channel.admit()?;
        channel
            .stream
            .write(&Ack {
                id: frame.id,
                digest: frame.payload_digest,
                outcome: StreamDeliveryOutcome::Acked { reply_body },
            })
            .await?;
    }
}

struct StreamPresence(crate::MeshState, String, [u8; 32]);
impl Drop for StreamPresence {
    fn drop(&mut self) {
        self.0.lock().detach_reverse_stream(&self.1, &self.2);
    }
}
