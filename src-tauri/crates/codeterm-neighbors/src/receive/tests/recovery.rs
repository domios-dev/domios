use super::*;
use crate::keys::{InstallationKeys, KeyRegistry, KeyScope, KeyStore, RecipientKeys, Slot};
use crate::supervisor::{supervise, InboxSession, ListenerState, ReceptionEvents};
use crate::wire::DeliveryReason;
use rand::{rngs::StdRng, SeedableRng};
use std::collections::{HashMap, VecDeque};

#[derive(Default)]
struct Vault {
    slots: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
    fail_current: Mutex<bool>,
    fail_candidate_delete: Mutex<bool>,
    events: std::sync::Arc<Mutex<Vec<&'static str>>>,
}
#[async_trait]
impl KeyStore for Vault {
    async fn load(&self, scope: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.slots.lock().unwrap().get(&scope.name(slot)).cloned())
    }
    async fn save(&self, scope: &KeyScope, slot: Slot, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        if matches!(slot, Slot::Current) && std::mem::take(&mut *self.fail_current.lock().unwrap())
        {
            return Err(Error::SecretStore);
        }
        if matches!(slot, Slot::Current) {
            self.events.lock().unwrap().push("current_saved");
        }
        self.slots.lock().unwrap().insert(scope.name(slot), bytes);
        Ok(())
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()> {
        if matches!(slot, Slot::Candidate)
            && std::mem::take(&mut *self.fail_candidate_delete.lock().unwrap())
        {
            return Err(Error::SecretStore);
        }
        self.slots.lock().unwrap().remove(&scope.name(slot));
        Ok(())
    }
}
struct Registry {
    uid: String,
    active: Mutex<Vec<String>>,
    receipt: Mutex<DeliveryResult>,
    uncertain: Mutex<bool>,
    events: std::sync::Arc<Mutex<Vec<&'static str>>>,
}
impl Registry {
    fn new(uid: &str, active: Vec<String>) -> Self {
        Self {
            uid: uid.into(),
            active: Mutex::new(active),
            receipt: Mutex::new(DeliveryStatus::Pending.into()),
            uncertain: Mutex::new(false),
            events: Default::default(),
        }
    }
}
#[async_trait]
impl KeyRegistry for Registry {
    async fn enroll(&self, signing: &str, _: &str, replace: bool) -> Result<String> {
        let id = format!(
            "vk_{}",
            &crate::crypto::sha256_hex(format!("verifier-key:{}:{signing}", self.uid).as_bytes())
                [..32]
        );
        let mut active = self.active.lock().unwrap();
        if replace {
            active.clear();
        }
        if !active.contains(&id) {
            active.push(id.clone());
        }
        Ok(id)
    }
    async fn revoke(&self, id: &str) -> Result<()> {
        self.active.lock().unwrap().retain(|other| other != id);
        self.events.lock().unwrap().push("old_revoked");
        *self.receipt.lock().unwrap() = DeliveryResult {
            status: DeliveryStatus::NotDelivered,
            reason: Some(DeliveryReason::KeyRevoked),
        };
        Ok(())
    }
    async fn active_key_ids(&self) -> Result<Vec<String>> {
        if *self.uncertain.lock().unwrap() {
            return Err(Error::Transport);
        }
        Ok(self.active.lock().unwrap().clone())
    }
}

async fn rotation_crash(window: usize) {
    let message = message();
    let uid = &message.doc.envelope.recipient_uid;
    let old = PrivateKeys::generate(&mut StdRng::seed_from_u64(7));
    let candidate = recipient();
    let vault = Vault::default();
    let scope = KeyScope::installation("machine", uid).unwrap();
    vault
        .save(&scope, Slot::Current, old.storage_bytes())
        .await
        .unwrap();
    vault
        .save(&scope, Slot::Candidate, candidate.storage_bytes())
        .await
        .unwrap();
    let mut active = vec![candidate.verifier_key_id(uid)];
    if window != 1 {
        active.push(old.verifier_key_id(uid));
    }
    let mut registry = Registry::new(uid, active);
    registry.events = vault.events.clone();
    vault.events.lock().unwrap().clear();
    let lifecycle = InstallationKeys {
        store: &vault,
        registry: &registry,
        scope,
        uid,
    };
    let fixture = Fixture::new();
    // An old local key must hold the cursor, so recovery can admit the candidate message.
    assert_eq!(
        receiver(&fixture, &old, &message)
            .process(&message, NOW)
            .await,
        Err(Error::LocalKeyUnavailable)
    );
    assert!(fixture.cursor.lock().unwrap().is_none());
    if window == 2 {
        *vault.fail_current.lock().unwrap() = true;
        assert!(matches!(
            lifecycle.reconcile().await,
            Err(Error::SecretStore)
        ));
        assert!(vault
            .load(&lifecycle.scope, Slot::Candidate)
            .await
            .unwrap()
            .is_some());
        assert_eq!(
            lifecycle
                .decrypt_for(
                    &message.doc.envelope,
                    &fixture.peers().keys[0].encryption_public_key_spki_b64
                )
                .await
                .unwrap()
                .as_str(),
            vector()["body"].as_str().unwrap()
        );
    }
    if window == 3 {
        *vault.fail_candidate_delete.lock().unwrap() = true;
        assert!(matches!(
            lifecycle.reconcile().await,
            Err(Error::SecretStore)
        ));
        for slot in [Slot::Current, Slot::Candidate] {
            let bytes = vault.load(&lifecycle.scope, slot).await.unwrap().unwrap();
            assert_eq!(
                PrivateKeys::from_storage(&bytes)
                    .unwrap()
                    .encryption_key_id(),
                candidate.encryption_key_id()
            );
        }
        assert_eq!(
            *vault.events.lock().unwrap(),
            ["old_revoked", "current_saved"]
        );
        assert_eq!(
            registry.active_key_ids().await.unwrap(),
            [candidate.verifier_key_id(uid)]
        );
    }
    lifecycle.reconcile().await.unwrap().unwrap();
    assert_eq!(
        registry.active_key_ids().await.unwrap(),
        [candidate.verifier_key_id(uid)]
    );
    if window == 3 {
        assert_eq!(
            *registry.receipt.lock().unwrap(),
            DeliveryResult {
                status: DeliveryStatus::NotDelivered,
                reason: Some(DeliveryReason::KeyRevoked),
            }
        );
    }
    let mut receiver = Receiver {
        transport: &fixture,
        cursors: &fixture,
        gateway: &fixture,
        private_keys: &lifecycle,
        recipient_uid: uid,
        friendship_id: &message.doc.envelope.friendship_id,
    };
    assert_eq!(
        receiver.process(&message, NOW).await.unwrap(),
        ReceiveOutcome::Delivered
    );
    assert_eq!(*fixture.writes.lock().unwrap(), 1);
    assert_eq!(
        fixture
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "ack")
            .count(),
        1
    );
    assert!(fixture.cursor.lock().unwrap().is_some());
    assert!(vault
        .load(&lifecycle.scope, Slot::Candidate)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn crash_after_candidate_enrollment_recovers_to_gateway_and_ack() {
    rotation_crash(0).await;
}
#[tokio::test]
async fn crash_after_old_revoke_recovers_to_gateway_and_ack() {
    rotation_crash(1).await;
}
#[tokio::test]
async fn crash_before_current_persistence_recovers_to_gateway_and_ack() {
    rotation_crash(2).await;
}

#[tokio::test]
async fn crash_after_current_copy_before_candidate_cleanup_finishes_rotation_and_ack() {
    rotation_crash(3).await;
}

struct FailedSettings;
#[async_trait]
impl crate::keys::EnrollmentState for FailedSettings {
    async fn enable(&self) -> Result<()> {
        Err(Error::CursorStore)
    }
}

#[tokio::test]
async fn failed_initial_settings_write_keeps_registry_enrollment_visible() {
    let vault = Vault::default();
    let registry = Registry::new("owner", vec![]);
    let lifecycle = InstallationKeys {
        store: &vault,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    assert!(matches!(
        lifecycle
            .enroll_and_enable(true, &mut StdRng::seed_from_u64(4), &FailedSettings)
            .await,
        Err(Error::CursorStore)
    ));
    assert_eq!(registry.active_key_ids().await.unwrap().len(), 1);
    assert!(vault
        .load(&lifecycle.scope, Slot::Current)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn uncertain_inventory_keeps_both_secret_slots() {
    let vault = Vault::default();
    let registry = Registry::new("owner", vec![]);
    let scope = KeyScope::installation("machine", "owner").unwrap();
    for slot in [Slot::Current, Slot::Candidate] {
        vault
            .save(&scope, slot, recipient().storage_bytes())
            .await
            .unwrap();
    }
    *registry.uncertain.lock().unwrap() = true;
    let lifecycle = InstallationKeys {
        store: &vault,
        registry: &registry,
        scope,
        uid: "owner",
    };
    assert_eq!(lifecycle.revoke().await, Err(Error::Transport));
    assert_eq!(vault.slots.lock().unwrap().len(), 2);
}

struct Stream(VecDeque<Result<Option<InboxEvent>>>);
#[async_trait]
impl InboxListener for Stream {
    async fn next(&mut self) -> Result<Option<InboxEvent>> {
        self.0.pop_front().unwrap_or(Err(Error::Transport))
    }
}
struct Reconnect {
    fixture: Fixture,
    keys: PrivateKeys,
    opens: Mutex<Vec<Option<Cursor>>>,
    states: Mutex<Vec<ListenerState>>,
    empty_current: bool,
}
#[async_trait]
impl NeighborTransport for Reconnect {
    async fn send(&self, _: &SignedEnvelope) -> Result<DeliveryStatus> {
        Ok(DeliveryStatus::Pending)
    }
    async fn listen_from_cursor(
        &self,
        _: &str,
        cursor: Option<&Cursor>,
    ) -> Result<Box<dyn InboxListener>> {
        self.opens.lock().unwrap().push(cursor.cloned());
        if self.empty_current {
            return Ok(Box::new(Stream(VecDeque::from([Ok(Some(
                InboxEvent::Ready,
            ))]))));
        }
        Ok(Box::new(Stream(VecDeque::from([
            Ok(Some(InboxEvent::Messages(vec![message()]))),
            Err(Error::Transport),
        ]))))
    }
    async fn ack(&self, sender: &str, id: &str) -> Result<DeliveryResult> {
        self.fixture.ack(sender, id).await
    }
    async fn peer_keys(&self, _: &str) -> Result<PeerKeys> {
        Ok(self.fixture.peers())
    }
    async fn delivery_status(&self, _: &str) -> Result<DeliveryResult> {
        Ok(DeliveryStatus::Pending.into())
    }
}
#[async_trait]
impl InboxSession for Reconnect {
    async fn stopped(&self) {
        std::future::pending().await
    }
    async fn prepare(&self) -> Result<()> {
        Ok(())
    }
    async fn cursor(&self) -> Result<Option<Cursor>> {
        self.fixture.load("").await
    }
    async fn receive(&self, messages: &mut [InboxMessage]) -> Result<()> {
        let original = message();
        Receiver {
            transport: self,
            cursors: &self.fixture,
            gateway: &self.fixture,
            private_keys: &self.keys,
            recipient_uid: &original.doc.envelope.recipient_uid,
            friendship_id: &original.doc.envelope.friendship_id,
        }
        .catch_up(messages, NOW)
        .await?;
        Ok(())
    }
}
#[async_trait]
impl ReceptionEvents for Reconnect {
    fn state(&self, state: ListenerState) {
        self.states.lock().unwrap().push(state);
    }
    async fn retry_after_failure(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn stream_error_reopens_from_durable_cursor_and_admits_exactly_once() {
    let fixture = Reconnect {
        fixture: Fixture::new(),
        keys: recipient(),
        opens: Mutex::new(vec![]),
        states: Mutex::new(vec![]),
        empty_current: false,
    };
    assert_eq!(
        supervise(
            &fixture,
            &message().doc.envelope.friendship_id,
            &fixture,
            &fixture
        )
        .await,
        Err(Error::Transport)
    );
    let opens = fixture.opens.lock().unwrap();
    assert_eq!(opens.len(), 4);
    assert!(opens[0].is_none());
    assert!(opens[1..]
        .iter()
        .all(|cursor| *cursor == *fixture.fixture.cursor.lock().unwrap()));
    assert_eq!(*fixture.fixture.writes.lock().unwrap(), 1);
    assert_eq!(
        fixture
            .fixture
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "ack")
            .count(),
        1
    );
    assert!(fixture
        .states
        .lock()
        .unwrap()
        .contains(&ListenerState::Reconnecting));
    assert_eq!(
        fixture.states.lock().unwrap().last(),
        Some(&ListenerState::Stopped)
    );
}

#[tokio::test]
async fn empty_inbox_current_marks_receiving_without_admission_or_ack() {
    let fixture = Reconnect {
        fixture: Fixture::new(),
        keys: recipient(),
        opens: Mutex::new(vec![]),
        states: Mutex::new(vec![]),
        empty_current: true,
    };
    assert_eq!(
        supervise(
            &fixture,
            &message().doc.envelope.friendship_id,
            &fixture,
            &fixture
        )
        .await,
        Err(Error::Transport)
    );
    assert!(fixture
        .states
        .lock()
        .unwrap()
        .windows(2)
        .any(|states| states == [ListenerState::Connecting, ListenerState::Receiving]));
    assert_eq!(*fixture.fixture.writes.lock().unwrap(), 0);
    assert!(fixture.fixture.events.lock().unwrap().is_empty());
    assert!(fixture.fixture.cursor.lock().unwrap().is_none());
}

struct RecoveryEvents {
    retry: tokio::sync::Semaphore,
    states: tokio::sync::mpsc::UnboundedSender<ListenerState>,
}
#[async_trait]
impl ReceptionEvents for RecoveryEvents {
    fn state(&self, state: ListenerState) {
        self.states.send(state).unwrap();
    }
    async fn retry_after_failure(&self, _: usize) -> Result<()> {
        self.retry
            .acquire()
            .await
            .map_err(|_| Error::Transport)?
            .forget();
        Ok(())
    }
}
#[tokio::test]
async fn reconnect_opens_only_after_explicit_recovery_event() {
    use std::sync::Arc;
    let fixture = Arc::new(Reconnect {
        fixture: Fixture::new(),
        keys: recipient(),
        opens: Mutex::new(vec![]),
        states: Mutex::new(vec![]),
        empty_current: true,
    });
    let (states, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let events = Arc::new(RecoveryEvents {
        retry: tokio::sync::Semaphore::new(0),
        states,
    });
    let task = {
        let fixture = fixture.clone();
        let events = events.clone();
        tokio::spawn(async move {
            supervise(
                fixture.as_ref(),
                &message().doc.envelope.friendship_id,
                fixture.as_ref(),
                events.as_ref(),
            )
            .await
        })
    };
    for expected in [
        ListenerState::Connecting,
        ListenerState::Receiving,
        ListenerState::Reconnecting,
    ] {
        assert_eq!(observed.recv().await, Some(expected));
    }
    assert_eq!(fixture.opens.lock().unwrap().len(), 1);
    events.retry.add_permits(1);
    for expected in [
        ListenerState::Connecting,
        ListenerState::Receiving,
        ListenerState::Reconnecting,
    ] {
        assert_eq!(observed.recv().await, Some(expected));
    }
    assert_eq!(fixture.opens.lock().unwrap().len(), 2);
    events.retry.close();
    assert_eq!(task.await.unwrap(), Err(Error::Transport));
    assert_eq!(observed.recv().await, Some(ListenerState::Stopped));
}

struct HeldAdmission {
    stopped: tokio::sync::watch::Receiver<bool>,
    admitted: tokio::sync::mpsc::UnboundedSender<()>,
    release: tokio::sync::Semaphore,
    persisted: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl InboxSession for HeldAdmission {
    async fn stopped(&self) {
        let mut stopped = self.stopped.clone();
        if *stopped.borrow_and_update() {
            return;
        }
        let _ = stopped.changed().await;
    }
    async fn prepare(&self) -> Result<()> {
        Ok(())
    }
    async fn cursor(&self) -> Result<Option<Cursor>> {
        Ok(None)
    }
    async fn receive(&self, _: &mut [InboxMessage]) -> Result<()> {
        self.admitted.send(()).unwrap();
        self.release
            .acquire()
            .await
            .map_err(|_| Error::Gateway)?
            .forget();
        self.persisted
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
}
#[tokio::test]
async fn shutdown_settles_inflight_admission_before_listener_exits() {
    use std::sync::{atomic::Ordering, Arc};
    let fixture = Arc::new(Reconnect {
        fixture: Fixture::new(),
        keys: recipient(),
        opens: Mutex::new(vec![]),
        states: Mutex::new(vec![]),
        empty_current: false,
    });
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let (admitted, mut entered) = tokio::sync::mpsc::unbounded_channel();
    let session = Arc::new(HeldAdmission {
        stopped,
        admitted,
        release: tokio::sync::Semaphore::new(0),
        persisted: false.into(),
    });
    let task = {
        let fixture = fixture.clone();
        let session = session.clone();
        tokio::spawn(async move {
            supervise(
                fixture.as_ref(),
                &message().doc.envelope.friendship_id,
                session.as_ref(),
                fixture.as_ref(),
            )
            .await
        })
    };
    entered.recv().await.unwrap();
    stop.send(true).unwrap();
    assert!(!task.is_finished());
    assert!(!session.persisted.load(Ordering::Acquire));
    session.release.add_permits(1);
    assert_eq!(task.await.unwrap(), Err(Error::Revoked));
    assert!(session.persisted.load(Ordering::Acquire));
    assert_eq!(fixture.opens.lock().unwrap().len(), 1);
}
