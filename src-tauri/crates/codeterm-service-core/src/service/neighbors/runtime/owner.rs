use super::*;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

pub(super) enum Operation {
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
    ReceiverChanged,
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
}

pub(super) struct Request {
    port: Box<dyn OperationPort>,
    generation: u64,
    operation: Operation,
    reply: oneshot::Sender<Result<Outcome, String>>,
}

impl ServiceContext {
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
            Operation::ReceiverChanged | Operation::RegistryChanged => {
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
                .neighbor_runtime_owned(command, caller.as_deref(), nonce.as_deref(), generation)
                .await
                .map(Outcome::Value),
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
