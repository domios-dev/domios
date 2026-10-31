use super::*;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

#[cfg(test)]
struct Observer(mpsc::UnboundedSender<&'static str>);

pub(super) enum Operation {
    ConfirmReceiver(Box<ReceiverCommit>),
    StopLocal {
        expected_generation: u64,
    },
    Read {
        friendship_id: String,
        through_message_id: String,
        expected_generation: u64,
    },
    Files {
        command: super::files::Command,
        caller: Option<String>,
        nonce: Option<String>,
    },
    Command {
        command: NeighborRuntimeCommand,
        caller: Option<String>,
        nonce: Option<String>,
    },
    RegistryChanged,
    Refuse {
        friendship: String,
        message_id: String,
        listener_generation: u64,
    },
    #[cfg(not(any(test, feature = "test-support")))]
    Restore,
}

pub(super) enum Outcome {
    Value(Value),
    ReceiverCommitted(Box<ReceiverCommit>),
}

pub(super) struct Continuation {
    pub command: NeighborRuntimeCommand,
    pub caller: Option<String>,
    pub nonce: Option<String>,
}

pub(super) struct ReceiverCommit {
    pub committed: receiver::selection::Committed,
    pub continuation: Option<Continuation>,
}

pub(super) struct Request {
    port: Box<dyn OperationPort>,
    generation: u64,
    operation: Operation,
    reply: oneshot::Sender<Result<Outcome, String>>,
}

impl ServiceContext {
    pub(super) async fn confirm_neighbor_receiver(&self, phase: Box<ReceiverCommit>) -> Result<Value, String> {
        use super::super::projection::ReceiverConfirmation;
        #[cfg(test)]
        self.observe_neighbor_operation("confirmation_waiting");
        let observed = self.wait_neighbor_projection_receiver(
            &phase.committed.registry.uid, phase.committed.login_generation,
            &phase.committed.friendship, phase.committed.revision,
        ).await;
        let explicit = phase.continuation.is_none();
        let result = phase.committed.result.clone();
        if observed != Ok(ReceiverConfirmation::Observed) {
            log::debug!("[neighbors] receiver assignment committed; projection confirmation pending: {observed:?}");
            return if explicit { Ok(result) } else { Err(receiver_preparation_pending()) };
        }
        let confirmation = self.neighbor_operation(Operation::ConfirmReceiver(phase)).await;
        match confirmation {
            Ok(Outcome::Value(value)) => Ok(value),
            Ok(Outcome::ReceiverCommitted(_)) => Err("Invalid Neighbor confirmation phase".into()),
            Err(error) if explicit => {
                log::debug!("[neighbors] receiver assignment committed; local reconciliation pending: {error}");
                Ok(result)
            }
            Err(error) => {
                log::debug!("[neighbors] receiver assignment committed; original action not resumed: {error}");
                Err(format!("Neighbor receiver assignment committed; local preparation unavailable: {error}"))
            }
        }
    }
    #[cfg(test)]
    pub(in crate::service::neighbors) fn observe_neighbor_owner_for_test(
        &self,
        sender: mpsc::UnboundedSender<&'static str>,
    ) {
        self.extensions.get_or_init(|| Observer(sender));
    }

    #[cfg(test)]
    fn observe_neighbor_operation(&self, event: &'static str) {
        if let Some(observer) = self.try_ext::<Observer>() {
            let _ = observer.0.send(event);
        }
    }
    pub(in crate::service::neighbors) fn require_neighbor_generation(
        &self,
        generation: u64,
    ) -> Result<(), String> {
        if self.neighbor_cell().generation.load(Ordering::Acquire) != generation {
            return Err("Neighbor account changed".into());
        }
        Ok(())
    }

    pub(super) async fn neighbor_operation(&self, operation: Operation) -> Result<Outcome, String> {
        #[cfg(test)]
        let registry_changed = matches!(&operation, Operation::RegistryChanged);
        let (reply, response) = oneshot::channel();
        {
            let mut owner = self.neighbor_cell().owner.lock();
            let sender = owner.get_or_insert_with(|| {
                let (sender, requests) = mpsc::channel::<Request>(32);
                tokio::spawn(run(requests));
                sender
            });
            sender
                .try_send(Request {
                    port: Box::new(self.clone()),
                    generation: self.neighbor_cell().generation.load(Ordering::Acquire),
                    operation,
                    reply,
                })
                .map_err(|_| "Neighbor operation queue unavailable")?;
        }
        #[cfg(test)]
        if registry_changed {
            self.observe_neighbor_operation("activation_queued");
        }
        response
            .await
            .map_err(|_| "Neighbor operation interrupted")?
    }

    async fn run_neighbor_operation(
        &self,
        generation: u64,
        operation: Operation,
    ) -> Result<Outcome, String> {
        self.require_neighbor_generation(generation)?;
        match operation {
            Operation::ConfirmReceiver(phase) => {
                if let Some(next) = &phase.continuation {
                    self.authorize_neighbor_runtime(&next.command, next.caller.as_deref(), next.nonce.as_deref()).await?;
                }
                let observed = self.reconcile_committed_neighbor_receiver(&phase.committed, phase.continuation.is_some()).await;
                if observed != Ok(super::super::projection::ReceiverConfirmation::Observed) {
                    log::debug!("[neighbors] receiver assignment committed; owned confirmation pending: {observed:?}");
                    return if phase.continuation.is_none() {
                        Ok(Outcome::Value(phase.committed.result))
                    } else {
                        Err(receiver_preparation_pending())
                    };
                }
                if let Some(next) = phase.continuation {
                    self.require_neighbor_friend_receiver(&phase.committed.friendship, Some(phase.committed.revision))
                        .map_err(|error| format!("Neighbor receiver assignment committed; local preparation unavailable: {error}"))?;
                    self.neighbor_runtime_owned(next.command, next.caller.as_deref(), next.nonce.as_deref(), phase.committed.generation, Some(&phase.committed)).await
                } else {
                    Ok(Outcome::Value(phase.committed.result))
                }
            }
            Operation::StopLocal {
                expected_generation,
            } => {
                self.require_neighbor_generation(expected_generation)?;
                self.drain_neighbor_stop_local(generation).await?;
                Ok(Outcome::Value(Value::Null))
            }
            Operation::Read {
                friendship_id,
                through_message_id,
                expected_generation,
            } => {
                self.require_neighbor_generation(expected_generation)?;
                self.neighbor_read_owned(friendship_id, through_message_id, generation)
                    .await
                    .map(Outcome::Value)
            }
            Operation::Files {
                command,
                caller,
                nonce,
            } => self
                .neighbor_files_owned(command, caller.as_deref(), nonce.as_deref(), generation)
                .await
                .map(Outcome::Value),
            Operation::RegistryChanged => {
                #[cfg(test)]
                self.observe_neighbor_operation("activation_admitted");
                let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
                self.start_neighbor_listeners(
                    transport.clone(),
                    registry,
                    scope,
                    metadata,
                    generation,
                )
                .await?;
                Ok(Outcome::Value(Value::Null))
            }
            Operation::Command {
                command,
                caller,
                nonce,
            } => self
                .neighbor_runtime_owned(command, caller.as_deref(), nonce.as_deref(), generation, None)
                .await,
            Operation::Refuse {
                friendship,
                message_id,
                listener_generation,
            } => {
                if self
                    .neighbor_cell()
                    .listener_generation
                    .load(Ordering::Acquire)
                    != listener_generation
                {
                    return Err("Neighbor listener changed".into());
                }
                let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
                self.require_neighbor_generation(generation)?;
                let lifecycle = InstallationKeys {
                    store: &self.neighbor_key_store().map_err(|e| e.to_string())?,
                    registry: registry.as_ref(),
                    scope,
                    uid: &transport.uid,
                };
                let author = self
                    .neighbor_author(
                        metadata
                            .settings()
                            .await
                            .map_err(|e| e.to_string())?
                            .bindings
                            .get(&friendship)
                            .map(String::as_str),
                    )
                    .map_err(|e| e.to_string())?;
                self.send_neighbor(
                    transport.as_ref(),
                    &lifecycle,
                    &metadata,
                    Outgoing {
                        authority: authority::SendAuthority::FriendshipGrant,
                        friendship,
                        kind: Kind::Refusal,
                        body: OutboundBody::Refusal,
                        in_reply_to: Some(message_id),
                        author,
                    },
                )
                .await
                .map(Outcome::Value)
                .map_err(|e| e.to_string())
            }
            #[cfg(not(any(test, feature = "test-support")))]
            Operation::Restore => {
                let (transport, registry, scope, metadata) = self.neighbor_ports().await?;
                self.start_neighbor_listeners(transport, registry, scope, metadata, generation)
                    .await?;
                Ok(Outcome::Value(Value::Null))
            }
        }
    }
}

fn receiver_preparation_pending() -> String {
    "Neighbor receiver assignment committed; local preparation pending. Do not repeat receiver selection.".into()
}

#[async_trait]
trait OperationPort: Send + Sync {
    async fn execute(&self, generation: u64, operation: Operation) -> Result<Outcome, String>;
}
#[async_trait]
impl OperationPort for ServiceContext {
    async fn execute(&self, generation: u64, operation: Operation) -> Result<Outcome, String> {
        self.run_neighbor_operation(generation, operation).await
    }
}
async fn run(mut requests: mpsc::Receiver<Request>) {
    while let Some(request) = requests.recv().await {
        let result = request
            .port
            .execute(request.generation, request.operation)
            .await;
        let _ = request.reply.send(result);
    }
}

#[cfg(test)]
mod tests;
