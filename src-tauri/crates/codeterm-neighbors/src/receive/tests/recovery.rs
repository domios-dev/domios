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
        self.slots.lock().unwrap().insert(scope.name(slot), bytes);
        Ok(())
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()> {
        self.slots.lock().unwrap().remove(&scope.name(slot));
        Ok(())
    }
}
struct Registry {
    uid: String,
    active: Mutex<Vec<String>>,
    receipt: Mutex<DeliveryResult>,
    uncertain: Mutex<bool>,
}
impl Registry {
    fn new(uid: &str, active: Vec<String>) -> Self {
        Self {
            uid: uid.into(),
            active: Mutex::new(active),
            receipt: Mutex::new(DeliveryStatus::Pending.into()),
            uncertain: Mutex::new(false),
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
    let registry = Registry::new(uid, active);
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
    lifecycle.reconcile().await.unwrap().unwrap();
    assert_eq!(
        registry.active_key_ids().await.unwrap(),
        [candidate.verifier_key_id(uid)]
    );
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

struct FailedSettings;
#[async_trait]
impl crate::keys::EnrollmentState for FailedSettings {
    async fn enable(&self) -> Result<()> {
        Err(Error::CursorStore)
    }
}

#[tokio::test]
async fn failed_initial_settings_write_signout_revokes_inventory_before_credentials() {
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
    // No settings flag participates in revocation, including keys absent from this vault.
    registry
        .active
        .lock()
        .unwrap()
        .push("vk_inventory_only".into());
    lifecycle.revoke().await.unwrap();
    let delete_credentials = || {
        assert!(registry.active.lock().unwrap().is_empty());
        assert_eq!(
            *registry.receipt.lock().unwrap(),
            DeliveryResult {
                status: DeliveryStatus::NotDelivered,
                reason: Some(DeliveryReason::KeyRevoked)
            }
        );
        assert!(vault.slots.lock().unwrap().is_empty());
    };
    delete_credentials();
}

#[tokio::test]
async fn signout_inventory_uncertainty_keeps_both_secret_slots() {
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

struct Stream(VecDeque<Result<Option<Vec<InboxMessage>>>>);
#[async_trait]
impl InboxListener for Stream {
    async fn next(&mut self) -> Result<Option<Vec<InboxMessage>>> {
        self.0.pop_front().unwrap_or(Err(Error::Transport))
    }
}
struct Reconnect {
    fixture: Fixture,
    keys: PrivateKeys,
    opens: Mutex<Vec<Option<Cursor>>>,
    states: Mutex<Vec<ListenerState>>,
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
        Ok(Box::new(Stream(VecDeque::from([
            Ok(Some(vec![message()])),
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
