use super::*;

struct Port {
    started: mpsc::UnboundedSender<&'static str>,
    release: Arc<tokio::sync::Semaphore>,
}

#[async_trait]
impl OperationPort for Port {
    async fn execute(&self, _: u64, operation: Operation) -> Result<Outcome, String> {
        match operation {
            Operation::Command {
                command: NeighborRuntimeCommand::Rotate,
                ..
            } => {
                self.started.send("rotation_started").unwrap();
                self.release.acquire().await.unwrap().forget();
                self.started.send("rotation_committed").unwrap();
            }
            Operation::Command {
                command: NeighborRuntimeCommand::Revoke,
                ..
            } => {
                self.started.send("revoke").unwrap();
            }
            _ => panic!("unexpected operation"),
        }
        Ok(Outcome::Value(Value::Null))
    }
}

#[tokio::test]
async fn operation_owner_finishes_rotation_before_revoke_even_when_caller_disconnects() {
    let (sender, requests) = mpsc::channel(32);
    let owner = tokio::spawn(run(requests));
    let (started, mut events) = mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let (reply, abandoned) = oneshot::channel();
    sender
        .try_send(Request {
            port: Box::new(Port {
                started: started.clone(),
                release: release.clone(),
            }),
            generation: 0,
            operation: Operation::Command {
                command: NeighborRuntimeCommand::Rotate,
                caller: None,
                nonce: None,
            },
            reply,
        })
        .unwrap_or_else(|_| panic!("owner closed"));
    assert_eq!(events.recv().await, Some("rotation_started"));
    drop(abandoned);
    let (reply, revoke) = oneshot::channel();
    sender
        .try_send(Request {
            port: Box::new(Port {
                started,
                release: release.clone(),
            }),
            generation: 0,
            operation: Operation::Command {
                command: NeighborRuntimeCommand::Revoke,
                caller: None,
                nonce: None,
            },
            reply,
        })
        .unwrap_or_else(|_| panic!("owner closed"));
    assert!(events.try_recv().is_err());
    release.add_permits(1);
    revoke.await.unwrap().unwrap();
    assert_eq!(events.recv().await, Some("rotation_committed"));
    assert_eq!(events.recv().await, Some("revoke"));
    drop(sender);
    owner.await.unwrap();
}

#[test]
fn identity_change_invalidates_old_operations_and_listener_generations() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    assert!(service.require_neighbor_generation(0).is_ok());
    service.neighbor_identity_changed();
    assert!(service.require_neighbor_generation(0).is_err());
    assert!(service.require_neighbor_generation(1).is_ok());
    assert_eq!(
        service
            .neighbor_cell()
            .listener_generation
            .load(Ordering::Acquire),
        1
    );
}

#[tokio::test]
async fn disabling_neighbors_stops_the_owned_inbox_listener() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let (shutdown, mut stopped) = tokio::sync::watch::channel(false);
    let (finished, completion) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        stopped.changed().await.unwrap();
        assert!(*stopped.borrow());
        let _ = finished.send(());
    });
    fixture
        .service
        .neighbor_cell()
        .listeners
        .lock()
        .active
        .insert(
            "f".into(),
            listener::Handle {
                gateway_tab: None,
                task,
                shutdown,
                retry: Arc::new(tokio::sync::Notify::new()),
            },
        );
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    fixture.service.neighbors_config_changed();
    tokio::time::timeout(std::time::Duration::from_secs(1), completion)
        .await
        .unwrap()
        .unwrap();
    assert!(fixture
        .service
        .neighbor_cell()
        .listeners
        .lock()
        .active
        .is_empty());
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    fixture.service.neighbors_config_changed();
    assert!(fixture.service.require_neighbors_enabled().is_ok());
    assert!(!fixture
        .service
        .neighbor_cell()
        .bootstrapped
        .load(Ordering::Acquire));
}
