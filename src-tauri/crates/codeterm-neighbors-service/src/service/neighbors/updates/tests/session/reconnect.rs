use super::*;

struct RefreshStream(Option<StateEvent>);

#[async_trait::async_trait]
impl StateListener for RefreshStream {
    async fn next(&mut self) -> Result<Option<StateEvent>> {
        match self.0.take() {
            Some(event) => Ok(Some(event)),
            None => std::future::pending().await,
        }
    }
}

struct RefreshPorts {
    opened: AtomicUsize,
    activated: tokio::sync::mpsc::UnboundedSender<u64>,
}

#[async_trait::async_trait]
impl SessionPorts for RefreshPorts {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(RefreshStream(Some(StateEvent::Snapshot(
            snapshot(27, Some("fixture-machine")),
        )))))
    }

    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        Ok(Box::new(Signals(
            Arc::new(AtomicUsize::new(0)),
            Arc::new(tokio::sync::Notify::new()),
        )))
    }

    async fn reconcile(
        &self,
        _: &Session,
        _: Value,
        _: Option<Value>,
        _: &mut notifications::Seen,
    ) -> Result<()> {
        Ok(())
    }

    async fn activate(&self, session: &Session, list: &Value) -> Result<()> {
        session
            .require_activation(list["version"].as_u64().unwrap())
            .await?;
        let receiver = &list["friends"][0]["my_receiver"];
        assert_eq!(receiver["enc_key_id"], "key_active");
        self.activated
            .send(receiver["revision"].as_u64().unwrap())
            .unwrap();
        Ok(())
    }
}

#[tokio::test]
async fn grant_listener_rearm_refresh_reopens_idle_watch_at_same_receiver_revision() {
    let (fixture, mut session, _, _, _) = fixture(Mode::Controlled).await;
    let (activated, mut activations) = tokio::sync::mpsc::unbounded_channel();
    let ports = Arc::new(RefreshPorts {
        opened: AtomicUsize::new(0),
        activated,
    });
    session.ports = ports.clone();
    let active = tokio::spawn(async move { session.listen().await });
    let verified = tokio::time::timeout(Duration::from_secs(5), async {
        assert_eq!(activations.recv().await, Some(27));
        fixture.service.refresh_neighbor_list().await.unwrap();
        assert_eq!(activations.recv().await, Some(27));
        assert_eq!(ports.opened.load(Ordering::SeqCst), 2);
        assert_eq!(
            fixture.service.neighbor_projection_diagnostics().await["dirty"],
            false
        );
    })
    .await;
    active.abort();
    assert!(active.await.unwrap_err().is_cancelled());
    verified.unwrap();
}

#[tokio::test]
async fn dirty_same_version_activation_remains_retryable_and_reconnect_recovers() {
    let (f, session, ports, _, _) = fixture(Mode::Controlled).await;
    let mut seen = notifications::Seen::default();
    session.project(snapshot(1, None), &mut seen).await.unwrap();
    f.service.invalidate_neighbor_list().await.unwrap();
    let error = session
        .project(snapshot(1, None), &mut seen)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        Error::LocalFence {
            phase: "activation_pending"
        }
    );
    assert!(codeterm_neighbors::supervisor::retryable(error));
    ReceptionEvents::failure(&session, error);
    session.project(snapshot(1, None), &mut seen).await.unwrap();
    assert_eq!(ports.activations.load(Ordering::SeqCst), 2);
    let diagnostics = f.service.neighbor_projection_diagnostics().await;
    assert_eq!(diagnostics["session_matches"], true);
    assert_eq!(diagnostics["dirty"], false);
    assert_eq!(diagnostics["available"], true);
    assert_eq!(
        diagnostics["last_watch_failure"],
        json!({
            "kind":"local_fence","operation":"activation_pending"
        })
    );
}

#[tokio::test]
async fn retired_account_and_update_generation_report_local_fences_without_key_revocation() {
    let (f, session, _, _, _) = fixture(Mode::Controlled).await;
    session.account.logout().await.unwrap();
    let error = session
        .project(snapshot(1, None), &mut notifications::Seen::default())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        Error::LocalFence {
            phase: "account_session"
        }
    );
    assert!(!codeterm_neighbors::supervisor::retryable(error));
    assert!(!error.to_string().contains("key_revoked"));
    assert_eq!(
        f.service.neighbor_projection_diagnostics().await["session_matches"],
        false
    );
    f.service
        .neighbor_updates()
        .generation
        .fetch_add(1, Ordering::AcqRel);
    assert_eq!(
        session.current(),
        Err(Error::LocalFence {
            phase: "update_generation"
        })
    );
}

struct TransientActivationPorts(AtomicUsize);

#[async_trait::async_trait]
impl SessionPorts for TransientActivationPorts {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        panic!("Snapshots are injected directly")
    }

    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        panic!("Registry events are not needed")
    }

    async fn activate(&self, session: &Session, _: &Value) -> Result<()> {
        let generation = session
            .service
            .neighbor_cell()
            .generation
            .load(Ordering::Acquire);
        let failure = (self.0.fetch_add(1, Ordering::SeqCst) == 0).then(|| Error::Transport.into());
        session
            .service
            .record_neighbor_activation(generation, failure);
        Ok(())
    }
}

#[tokio::test]
async fn transient_activation_waits_for_the_next_snapshot_without_completing_or_scheduling_retry() {
    let (f, mut session, _, _, mut deadlines) = fixture(Mode::Controlled).await;
    let ports = Arc::new(TransientActivationPorts(AtomicUsize::new(0)));
    session.ports = ports.clone();
    let mut seen = notifications::Seen::default();
    session.project(snapshot(1, None), &mut seen).await.unwrap();
    assert!(f.service.neighbor_activation_stale());
    assert!(!f.service.neighbor_projection_activated(
        &session.transport.uid,
        session.expected.login_generation,
        1
    ));
    assert_eq!(ports.0.load(Ordering::SeqCst), 1);
    assert!(deadlines.try_recv().is_err());
    session.project(snapshot(1, None), &mut seen).await.unwrap();
    assert!(!f.service.neighbor_activation_stale());
    assert!(f.service.neighbor_projection_activated(
        &session.transport.uid,
        session.expected.login_generation,
        1
    ));
    assert_eq!(ports.0.load(Ordering::SeqCst), 2);
    assert!(deadlines.try_recv().is_err());
}
