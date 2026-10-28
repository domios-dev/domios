use super::*;
use crate::adapter::{AuthenticatedCall, Enrollment, FirebaseRegistry};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct Receiver {
    installation: String,
    key: String,
    revision: u64,
}

struct Calls {
    receiver: Mutex<Receiver>,
    revoked: Option<String>,
    inventory_unavailable: bool,
    enrollments: Mutex<Vec<Value>>,
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        panic!("fixture must not authenticate")
    }

    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        let mut receiver = self.receiver.lock().unwrap();
        match route {
            "friendsReceiverStatus" => Ok(json!({
                "receiver_revision":receiver.revision,"receiver_installation_id":receiver.installation,
                "receiver_key_id":receiver.key,"receiver_machine_label":"Desk",
                "receiving_enabled":true,"is_me":data["installation_id"]==receiver.installation,
            })),
            "friendsOwnKeys" if self.inventory_unavailable => Err(Error::Transport),
            "friendsOwnKeys" => Ok(json!([{"key_id":receiver.key}])),
            "friendsEnrollVerifierKey" => {
                self.enrollments.lock().unwrap().push(data.clone());
                let key = format!(
                    "vk_{}",
                    &sha256_hex(
                        format!(
                            "verifier-key:owner:{}",
                            data["public_key_spki_b64"].as_str().unwrap()
                        )
                        .as_bytes()
                    )[..32]
                );
                assert_ne!(
                    Some(key.as_str()),
                    self.revoked.as_deref(),
                    "never submit a revoked key for re-enrollment"
                );
                if data["expected_receiver_revision"] != receiver.revision {
                    return Err(Error::ReceiverRevisionStale);
                }
                assert_eq!(data["takeover"], true);
                assert_eq!(data["features"], json!(["files.v1"]));
                receiver.installation = data["installation_id"].as_str().unwrap().into();
                receiver.key = key;
                receiver.revision += 1;
                Ok(json!({"verifier_key_id":receiver.key,"receiver_revision":receiver.revision}))
            }
            _ => panic!("unexpected takeover fixture call {route}"),
        }
    }
}

#[derive(Default)]
struct Enabled(AtomicBool);

#[async_trait]
impl EnrollmentState for Enabled {
    async fn enable(&self) -> Result<()> {
        self.0.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::test]
async fn confirmed_takeover_key_matrix_preserves_active_keys_replaces_revoked_and_honors_revision()
{
    for case in [
        "first",
        "active",
        "revoked",
        "missing_local",
        "missing_registry",
    ] {
        for stale in [false, true] {
            let store = Store::default();
            let mut rng = StdRng::seed_from_u64(29);
            let old = PrivateKeys::generate(&mut rng);
            let other = PrivateKeys::generate(&mut rng);
            let scope = KeyScope::installation("returning", "owner").unwrap();
            let stored = matches!(case, "active" | "revoked" | "missing_registry");
            if stored {
                store
                    .save(&scope, Slot::Current, old.storage_bytes())
                    .await
                    .unwrap();
            }
            let receiver_here = matches!(case, "active" | "missing_local");
            let initial_key = if receiver_here {
                old.verifier_key_id("owner")
            } else {
                other.verifier_key_id("owner")
            };
            let calls = Arc::new(Calls {
                receiver: Mutex::new(Receiver {
                    installation: if receiver_here { "returning" } else { "other" }.into(),
                    key: initial_key.clone(),
                    revision: 7,
                }),
                revoked: (case == "revoked").then(|| old.verifier_key_id("owner")),
                inventory_unavailable: false,
                enrollments: Mutex::default(),
            });
            let registry = FirebaseRegistry {
                calls: calls.clone(),
                uid: "owner".into(),
                installation_id: "returning".into(),
                machine_label: "Desk".into(),
                enrollment: Some(Enrollment {
                    expected_revision: if stale { 6 } else { 7 },
                    takeover: true,
                    operation_id: None,
                }),
            };
            let keys = InstallationKeys {
                store: &store,
                registry: &registry,
                scope: scope.clone(),
                uid: "owner",
            };
            let enabled = Enabled::default();
            let result = keys.enroll_and_enable(true, &mut rng, &enabled).await;
            assert_eq!(
                calls.enrollments.lock().unwrap().len(),
                1,
                "{case} stale={stale}"
            );
            assert_eq!(enabled.0.load(Ordering::Relaxed), !stale);
            let status = registry.receiver_status().await.unwrap();
            if stale {
                assert!(matches!(result, Err(Error::ReceiverRevisionStale)));
                assert_eq!(status.receiver_revision, 7);
                assert_eq!(status.receiver_key_id, Some(initial_key));
                let current = keys.load().await.unwrap();
                assert_eq!(current.is_some(), stored);
                if let Some(current) = current {
                    assert_eq!(current.encryption_key_id(), old.encryption_key_id());
                }
            } else {
                let received = result.unwrap();
                assert!(status.receiving_here());
                assert_eq!(
                    status.receiver_key_id,
                    Some(received.verifier_key_id("owner"))
                );
                assert_eq!(
                    keys.load().await.unwrap().unwrap().encryption_key_id(),
                    received.encryption_key_id()
                );
                assert_eq!(
                    received.encryption_key_id() == old.encryption_key_id(),
                    case == "active"
                );
                assert!(store.load(&scope, Slot::Candidate).await.unwrap().is_none());
            }
        }
    }
}

#[tokio::test]
async fn unconfirmed_takeover_or_unavailable_inventory_never_replaces_the_stored_key() {
    let store = Store::default();
    let mut rng = StdRng::seed_from_u64(44);
    let old = PrivateKeys::generate(&mut rng);
    let scope = KeyScope::installation("returning", "owner").unwrap();
    store
        .save(&scope, Slot::Current, old.storage_bytes())
        .await
        .unwrap();
    let calls = Arc::new(Calls {
        receiver: Mutex::new(Receiver {
            installation: "other".into(),
            key: "vk_foreign".into(),
            revision: 7,
        }),
        revoked: Some(old.verifier_key_id("owner")),
        inventory_unavailable: true,
        enrollments: Mutex::default(),
    });
    let registry = FirebaseRegistry {
        calls: calls.clone(),
        uid: "owner".into(),
        installation_id: "returning".into(),
        machine_label: "Desk".into(),
        enrollment: Some(Enrollment {
            expected_revision: 7,
            takeover: true,
            operation_id: None,
        }),
    };
    let keys = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: scope.clone(),
        uid: "owner",
    };
    assert!(matches!(
        keys.enroll(false, &mut rng).await,
        Err(Error::Gateway)
    ));
    assert!(matches!(
        keys.enroll(true, &mut rng).await,
        Err(Error::Transport)
    ));
    assert!(calls.enrollments.lock().unwrap().is_empty());
    assert_eq!(
        keys.load().await.unwrap().unwrap().encryption_key_id(),
        old.encryption_key_id()
    );
    assert!(store.load(&scope, Slot::Candidate).await.unwrap().is_none());
}
