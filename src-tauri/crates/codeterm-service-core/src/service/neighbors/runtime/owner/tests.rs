use super::*;

mod file_authority;

#[tokio::test]
async fn public_empty_send_and_failed_pad_read_never_enter_owner_queue_or_network_dispatch() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let (sender, mut requests) = mpsc::channel::<Request>(1);
    *service.neighbor_cell().owner.lock() = Some(sender);
    let invalid_pad = fixture.temp.path().join("invalid-pad");
    std::fs::write(&invalid_pad, "opaque file").unwrap();
    assert!(std::fs::write(invalid_pad.join("reply.txt"), "opaque reply").is_err());
    let failed_read = std::fs::read_to_string(invalid_pad.join("reply.txt")).unwrap_or_default();
    for body in [
        String::new(),
        " \r\n\t".into(),
        "\u{2003}\u{3000}\u{a0}".into(),
        failed_read,
    ] {
        for kind in [
            Kind::Question,
            Kind::Answer,
            Kind::Refusal,
            Kind::TabRequest,
        ] {
            let command = NeighborRuntimeCommand::Send {
                friendship_id: "a".repeat(64),
                kind,
                body: body.clone(),
                in_reply_to: Some("b".repeat(32)),
            };
            tokio::select! {
                result = service.neighbor_runtime(command,None,None) => assert_eq!(result.unwrap_err(),Error::EmptyBody.to_string()),
                _ = requests.recv() => panic!("invalid text reached the owner queue; ports, keys and network must remain unreachable"),
            }
        }
    }
    assert!(matches!(
        requests.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let command = NeighborRuntimeCommand::Send {
        friendship_id: "a".repeat(64),
        kind: Kind::Question,
        body: "opaque nonempty".into(),
        in_reply_to: None,
    };
    let send = service.neighbor_runtime(command, None, None);
    tokio::pin!(send);
    let request = tokio::select! {
        result = &mut send => panic!("nonempty send bypassed queue sentinel: {result:?}"),
        request = requests.recv() => request.unwrap(),
    };
    request
        .reply
        .send(Err("opaque owner sentinel".into()))
        .unwrap_or_else(|_| panic!("caller abandoned queue"));
    assert_eq!(send.await.unwrap_err(), "opaque owner sentinel");
}

#[tokio::test]
async fn public_file_only_send_still_enters_its_distinct_owner_operation() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let (sender, mut requests) = mpsc::channel::<Request>(1);
    *service.neighbor_cell().owner.lock() = Some(sender);
    let send = service.neighbor_files(json!({"operation":"send","friendship_id":"a".repeat(64),"kind":"question","body":"","in_reply_to":null,"paths":["opaque-file"]}),None,None);
    tokio::pin!(send);
    let request = tokio::select! {
        result = &mut send => panic!("file-only send refused before queue sentinel: {result:?}"),
        request = requests.recv() => request.unwrap(),
    };
    assert!(
        matches!(&request.operation,Operation::Files {command:super::super::files::Command::Send {body,paths,..},caller:None,nonce:None} if body.is_empty() && paths.len() == 1)
    );
    request
        .reply
        .send(Ok(Outcome::Value(json!({"opaque":"queued"}))))
        .unwrap_or_else(|_| panic!("caller abandoned queue"));
    assert_eq!(send.await.unwrap(), json!({"opaque":"queued"}));
}

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
