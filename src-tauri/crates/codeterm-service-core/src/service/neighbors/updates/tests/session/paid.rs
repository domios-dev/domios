use super::*;
use crate::service::neighbors::paid_fixture::{reading, Paid};

struct LockedProjection;
#[async_trait::async_trait]
impl SessionPorts for LockedProjection {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        panic!("No live state listener")
    }
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        panic!("Locked reception must not open signals")
    }
}

#[tokio::test]
async fn authoritative_move_away_retires_failed_concierge_while_reception_is_locked() {
    use crate::multiplexer::types::{NeighborAuthority, TabBacking};
    for access in ["paid", "locked"] {
        let (fixture, mut session, _, calls, _) = fixture(Mode::Controlled).await;
        session.ports = Arc::new(LockedProjection);
        let paid = fixture.service.ext::<Paid>();
        paid.now.store(1_700_086_400, Ordering::SeqCst);
        *paid.provider.0.lock() = Err("offline".into());
        paid.billing
            .refresh(
                Some(&session.expected),
                codeterm_billing::RefreshEvent::Explicit,
                std::time::Instant::now() + Duration::from_secs(1),
            )
            .await;
        assert!(fixture.service.neighbor_access_locked().await);
        let friendship = "a".repeat(64);
        let (root, child, other) = {
            let mut mux = fixture.service.mux.write();
            let workspace = mux.list_workspaces()[0].id.clone();
            let other = mux.create_tab(&workspace).unwrap();
            let root = mux.create_tab(&workspace).unwrap();
            let child = mux.create_tab(&workspace).unwrap();
            for (id, parent) in [(&root, None), (&child, Some(root.clone()))] {
                let tab = mux.get_tab_mut(id).unwrap();
                tab.agent_type = Some("opaque-runtime".into());
                tab.agent_session_id = Some(format!("own-thread-{id}"));
                tab.session_binding_nonce = Some(format!("own-nonce-{id}"));
                tab.backing = TabBacking::ChatBackend {
                    plugin_id: "opaque-runtime".into(),
                    session_id: format!("own-thread-{id}"),
                };
                tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
                tab.agent_runtime_state =
                    Some(json!({"state":"degraded","failureCode":"provider_refused"}));
                tab.neighbor_authority = Some(NeighborAuthority {
                    uid: session.expected.identity.uid.clone(),
                    friendship_id: friendship.clone(),
                    root_tab_id: parent,
                });
            }
            (root, child, other)
        };
        fixture.install_seeded_storage_desk();
        let mut settings = session.metadata.settings().await.unwrap();
        settings.bindings.insert(friendship.clone(), root.clone());
        session.metadata.save_settings(settings).await.unwrap();
        fixture
            .service
            .set_neighbor_friend_receiver_for_test(&friendship, Some(true), 1);
        let mut value = snapshot(2, Some("other-machine")).list(1);
        value["access"] = json!(access);
        value["updated_at"] = json!(1);
        value["own_keys"] = json!([]);
        let projected = Snapshot::parse(value).unwrap();
        session
            .project(projected.clone(), &mut notifications::Seen::default())
            .await
            .unwrap();
        session
            .project(projected, &mut notifications::Seen::default())
            .await
            .unwrap();
        let saved = session.metadata.settings().await.unwrap();
        assert!(!saved.bindings.contains_key(&friendship));
        assert_eq!(
            saved.dormant_concierges[&friendship].provider.as_deref(),
            Some("opaque-runtime")
        );
        let mux = fixture.service.mux.read();
        assert!(mux.get_tab(&root).is_none());
        assert!(mux.get_tab(&child).is_none());
        assert!(mux.get_tab(&other).is_some());
        drop(mux);
        assert!(!fixture.service.neighbor_friend_receiving_here(&friendship));
        assert_eq!(fixture.service.neighbor_cell().listener_count_for_test(), 0);
        assert_eq!(calls.calls.load(Ordering::SeqCst), 0);
        assert!(fixture
            .ui
            .0
            .lock()
            .iter()
            .any(|(name, payload)| name == "neighbor-agent-changed"
                && payload["friendship_id"] == friendship
                && payload["concierge_dormant"] == true
                && payload["gateway_tab"].is_null()));
    }
}

struct State {
    snapshot: Option<Snapshot>,
    waiting: tokio::sync::mpsc::UnboundedSender<()>,
}
#[async_trait::async_trait]
impl StateListener for State {
    async fn next(&mut self) -> Result<Option<StateEvent>> {
        if let Some(snapshot) = self.snapshot.take() {
            return Ok(Some(StateEvent::Snapshot(snapshot)));
        }
        self.waiting.send(()).unwrap();
        std::future::pending().await
    }
}
struct Signal(tokio::sync::mpsc::UnboundedSender<()>);
impl Drop for Signal {
    fn drop(&mut self) {
        self.0.send(()).unwrap();
    }
}
#[async_trait::async_trait]
impl codeterm_neighbors::registry::RegistryListener for Signal {
    async fn next(&mut self) -> Result<Option<RegistrySignal>> {
        std::future::pending().await
    }
}
struct Ports {
    state: parking_lot::Mutex<Option<State>>,
    opened: tokio::sync::mpsc::UnboundedSender<()>,
    closed: tokio::sync::mpsc::UnboundedSender<()>,
}
#[async_trait::async_trait]
impl SessionPorts for Ports {
    async fn listen_state(&self) -> Result<Box<dyn StateListener>> {
        Ok(Box::new(self.state.lock().take().unwrap()))
    }
    async fn listen_signals(
        &self,
    ) -> Result<Box<dyn codeterm_neighbors::registry::RegistryListener>> {
        self.opened.send(()).unwrap();
        Ok(Box::new(Signal(self.closed.clone())))
    }
    async fn activate(&self, _: &Session, _: &Value) -> Result<()> {
        Ok(())
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
}

#[tokio::test]
async fn paid_signal_expiry_and_local_lock_close_without_a_state_event_and_upgrade_restarts() {
    for expiry in [true, false] {
        let (fixture, mut session, _, _, _) = fixture(Mode::Controlled).await;
        let paid = fixture.service.ext::<Paid>().clone();
        let (opened, mut opens) = tokio::sync::mpsc::unbounded_channel();
        let (closed, mut closes) = tokio::sync::mpsc::unbounded_channel();
        let (waiting, mut waits) = tokio::sync::mpsc::unbounded_channel();
        session.ports = Arc::new(Ports {
            state: parking_lot::Mutex::new(Some(State {
                snapshot: Some(snapshot(1, Some("fixture-machine"))),
                waiting,
            })),
            opened,
            closed,
        });
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        let control = async {
            opens.recv().await.unwrap();
            waits.recv().await.unwrap();
            if expiry {
                paid.now.store(1_700_086_400, Ordering::SeqCst);
                *paid.provider.0.lock() = Err("offline".into());
            } else {
                paid.now.store(1_700_000_120, Ordering::SeqCst);
                *paid.provider.0.lock() = Ok(reading(
                    &session.expected.identity.uid,
                    "signed-free-downgrade",
                    11,
                    0,
                ));
            }
            paid.billing
                .refresh(
                    Some(&session.expected),
                    codeterm_billing::RefreshEvent::Explicit,
                    std::time::Instant::now() + Duration::from_secs(1),
                )
                .await;
            assert!(paid
                .billing
                .neighbors_locked(&session.expected.identity.uid));
            fixture.service.neighbor_entitlements_changed(true);
            closes.recv().await.unwrap();
            waits.recv().await.unwrap();
            assert!(opens.try_recv().is_err());
            assert_eq!(
                fixture
                    .service
                    .require_neighbor_paid(&session.expected.identity.uid),
                Err(Error::Locked)
            );
            assert_eq!(
                fixture
                    .service
                    .peek_neighbor_projection(
                        &session.expected.identity.uid,
                        session.expected.login_generation,
                        "friendsList",
                        "fixture-machine"
                    )
                    .unwrap()["access"],
                "paid"
            );
            paid.now.fetch_add(60, Ordering::SeqCst);
            let shift = if expiry { 86_400 } else { 0 };
            *paid.provider.0.lock() = Ok(reading(
                &session.expected.identity.uid,
                "valid-paid",
                12,
                shift,
            ));
            paid.billing
                .refresh(
                    Some(&session.expected),
                    codeterm_billing::RefreshEvent::Explicit,
                    std::time::Instant::now() + Duration::from_secs(2),
                )
                .await;
            fixture.service.neighbor_entitlements_changed(true);
            opens.recv().await.unwrap();
            waits.recv().await.unwrap();
            assert!(fixture
                .service
                .require_neighbor_paid(&session.expected.identity.uid)
                .is_ok());
            stop.send(()).unwrap();
        };
        let run = async {
            tokio::select! { result = session.listen() => panic!("Free state watch stopped: {result:?}"), _ = &mut stopped => {} }
        };
        tokio::join!(run, control);
        closes.recv().await.unwrap();
    }
}
