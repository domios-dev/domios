use super::*;
use tokio::sync::{mpsc, oneshot};
use async_trait::async_trait;

pub(super) enum Operation {
    Command {
        command: NeighborRuntimeCommand,
        caller: Option<String>,
        nonce: Option<String>,
    },
    Refuse { friendship: String, message_id: String, listener_generation: u64 },
    #[cfg(not(any(test, feature = "test-support")))]
    Restore,
    Logout,
}

pub(super) struct Request {
    port: Box<dyn OperationPort>,
    generation: u64,
    operation: Operation,
    reply: oneshot::Sender<Result<Value, String>>,
}

impl ServiceContext {
    pub(super) fn require_neighbor_generation(&self, generation: u64) -> Result<(), String> {
        if self.neighbor_cell().generation.load(Ordering::Acquire) != generation {
            return Err("Neighbor account changed".into());
        }
        Ok(())
    }

    pub(super) async fn neighbor_operation(&self, operation: Operation) -> Result<Value, String> {
        let (reply, response) = oneshot::channel();
        {
            let mut owner = self.neighbor_cell().owner.lock();
            let sender = owner.get_or_insert_with(|| {
                let (sender, requests) = mpsc::unbounded_channel::<Request>();
                tokio::spawn(run(requests));
                sender
            });
            sender.send(Request {
                port: Box::new(self.clone()),
                generation: self.neighbor_cell().generation.load(Ordering::Acquire),
                operation,
                reply,
            }).map_err(|_| "Neighbor operation owner stopped")?;
        }
        response.await.map_err(|_| "Neighbor operation interrupted")?
    }

    async fn run_neighbor_operation(&self, generation: u64, operation: Operation) -> Result<Value, String> {
        self.require_neighbor_generation(generation)?;
        match operation {
            Operation::Command { command, caller, nonce } => self.neighbor_runtime_owned(command, caller.as_deref(), nonce.as_deref(), generation).await,
            Operation::Refuse { friendship, message_id, listener_generation } => {
                if self.neighbor_cell().listener_generation.load(Ordering::Acquire) != listener_generation { return Err("Neighbor listener changed".into()); }
                let (transport, scope, metadata) = self.neighbor_ports().await?;
                self.require_neighbor_generation(generation)?;
                let lifecycle = InstallationKeys { store: &OsKeyStore, registry: transport.as_ref(), scope, uid: &transport.uid };
                self.send_neighbor(transport.as_ref(), &lifecycle, &metadata, Outgoing {
                    friendship, kind: Kind::Answer, body: OutboundBody::Refusal, in_reply_to: Some(message_id),
                }).await.map_err(|e| e.to_string())
            }
            #[cfg(not(any(test, feature = "test-support")))]
            Operation::Restore => {
                let (transport, scope, metadata) = self.neighbor_ports().await?;
                self.start_neighbor_listeners(transport, scope, metadata, generation).await?;
                Ok(Value::Null)
            }
            Operation::Logout => self.revoke_neighbors_owned().await.map(|_| Value::Null),
        }
    }
}

#[async_trait]
trait OperationPort: Send + Sync {
    async fn execute(&self, generation: u64, operation: Operation) -> Result<Value, String>;
}
#[async_trait]
impl OperationPort for ServiceContext {
    async fn execute(&self, generation: u64, operation: Operation) -> Result<Value, String> {
        self.run_neighbor_operation(generation, operation).await
    }
}
async fn run(mut requests: mpsc::UnboundedReceiver<Request>) {
    while let Some(request) = requests.recv().await {
        let result = request.port.execute(request.generation, request.operation).await;
        let _ = request.reply.send(result);
    }
}

#[cfg(test)]
mod tests;
