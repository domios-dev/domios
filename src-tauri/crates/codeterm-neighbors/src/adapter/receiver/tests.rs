use super::*;
use crate::{
    crypto::PrivateKeys,
    keys::{InstallationKeys, KeyScope, KeyStore, Slot},
};
use rand::{rngs::StdRng, SeedableRng};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

struct Store {
    bytes: Zeroizing<Vec<u8>>,
    reads: AtomicUsize,
}

#[async_trait]
impl KeyStore for Store {
    async fn load(&self, _: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(match slot {
            Slot::Current => Some(self.bytes.clone()),
            Slot::Candidate => None,
        })
    }
    async fn save(&self, _: &KeyScope, _: Slot, _: Zeroizing<Vec<u8>>) -> Result<()> {
        panic!("legacy adoption must retain the existing key slots")
    }
    async fn delete(&self, _: &KeyScope, _: Slot) -> Result<()> {
        panic!("legacy adoption must not forget a local key")
    }
}

struct Calls {
    key_id: String,
    signing: String,
    encryption: String,
    adopted: AtomicBool,
    elsewhere: bool,
    uncertain: bool,
    enrollments: Mutex<Vec<Value>>,
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        match route {
            "friendsReceiverStatus" if self.elsewhere => Ok(json!({
                "receiver_revision":9,"receiver_installation_id":"other","receiver_key_id":self.key_id,
                "receiver_machine_label":"Other","receiving_enabled":true,"is_me":false,"legacy_active":false
            })),
            "friendsReceiverStatus" if self.adopted.load(Ordering::Relaxed) => Ok(json!({
                "receiver_revision":1,"receiver_installation_id":"machine","receiver_key_id":self.key_id,
                "receiver_machine_label":"Here","receiving_enabled":true,"is_me":true,"legacy_active":false
            })),
            "friendsReceiverStatus" => Ok(json!({
                "receiver_revision":0,"receiver_installation_id":null,"receiver_key_id":null,
                "receiver_machine_label":null,"receiving_enabled":false,"is_me":false,"legacy_active":true
            })),
            "friendsOwnKeys" => Ok(json!([{"key_id":self.key_id}])),
            "friendsEnrollVerifierKey" => {
                assert_eq!(data["public_key_spki_b64"], self.signing);
                assert_eq!(data["encryption_public_key_spki_b64"], self.encryption);
                assert_eq!(data["takeover"], false);
                assert_eq!(data["expected_receiver_revision"], 0);
                assert_eq!(data["installation_id"], "machine");
                assert_eq!(data["machine_label"], "Here");
                assert_eq!(data["operation_id"].as_str().unwrap().len(), 64);
                assert!(data.get("replace_others").is_none());
                self.enrollments.lock().unwrap().push(data);
                if self.uncertain {
                    return Err(Error::Transport);
                }
                self.adopted.store(true, Ordering::Relaxed);
                Ok(
                    json!({"verifier_key_id":self.key_id,"key_id":self.key_id,"revoked_key_ids":[],"receiver_revision":1}),
                )
            }
            _ => panic!("unexpected legacy adoption mutation: {route}"),
        }
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        Err(Error::Transport)
    }
}

fn fixture(
    matching: bool,
    elsewhere: bool,
    uncertain: bool,
) -> (FirebaseRegistry, Store, Arc<Calls>) {
    let keys = PrivateKeys::generate(&mut StdRng::seed_from_u64(42));
    let calls = Arc::new(Calls {
        key_id: if matching {
            keys.verifier_key_id("owner")
        } else {
            format!("vk_{}", "b".repeat(32))
        },
        signing: keys.signing_spki(),
        encryption: keys.encryption_spki(),
        adopted: AtomicBool::new(false),
        elsewhere,
        uncertain,
        enrollments: Mutex::new(Vec::new()),
    });
    let registry = FirebaseRegistry {
        calls: calls.clone(),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Here".into(),
        enrollment: None,
    };
    (
        registry,
        Store {
            bytes: keys.storage_bytes(),
            reads: AtomicUsize::new(0),
        },
        calls,
    )
}

#[tokio::test]
async fn legacy_local_active_key_adopts_once_with_the_same_key_and_no_takeover() {
    let (registry, store, calls) = fixture(true, false, false);
    let keys = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    for _ in 0..2 {
        let status = registry.receiver_status_for(&keys, true).await.unwrap();
        assert!(status.receiving_here());
        assert_eq!(status.receiver_revision, 1);
        assert_eq!(
            keys.load().await.unwrap().unwrap().verifier_key_id("owner"),
            calls.key_id
        );
    }
    assert_eq!(calls.enrollments.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn another_installations_key_or_receiver_never_gets_adopted() {
    for (matching, elsewhere) in [(false, false), (true, true)] {
        let (registry, store, calls) = fixture(matching, elsewhere, false);
        let keys = InstallationKeys {
            store: &store,
            registry: &registry,
            scope: KeyScope::installation("machine", "owner").unwrap(),
            uid: "owner",
        };
        let status = registry.receiver_status_for(&keys, true).await.unwrap();
        assert!(!status.receiving_here());
        assert!(calls.enrollments.lock().unwrap().is_empty());
        if elsewhere {
            assert_eq!(store.reads.load(Ordering::Relaxed), 0);
        }
    }
}

#[tokio::test]
async fn uncertain_legacy_adoption_keeps_receiving_without_replaying_the_call() {
    let (registry, store, calls) = fixture(true, false, true);
    let keys = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let status = registry.receiver_status_for(&keys, true).await.unwrap();
    assert!(status.receiving_here());
    assert_eq!(status.receiver_revision, 0);
    assert_eq!(calls.enrollments.lock().unwrap().len(), 1);
    assert!(registry
        .receiver_status_for(&keys, false)
        .await
        .unwrap()
        .receiving_here());
    assert_eq!(calls.enrollments.lock().unwrap().len(), 1);
}
