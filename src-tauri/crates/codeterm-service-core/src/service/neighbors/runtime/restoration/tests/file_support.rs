use super::*;
use async_trait::async_trait;
use codeterm_neighbors::{adapter::AuthenticatedCall, crypto::PrivateKeys};
use std::sync::Mutex;

struct Calls {
    status: Value,
    enrollments: Mutex<Vec<Value>>,
    uncertain: bool,
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("no network")
    }
    async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
        match route {
            "friendsReceiverStatus" => Ok(self.status.clone()),
            "friendsEnrollVerifierKey" => {
                self.enrollments.lock().unwrap().push(data);
                if self.uncertain {
                    return Err(Error::Transport);
                }
                Ok(json!({"verifier_key_id":self.status["receiver_key_id"]}))
            }
            _ => panic!("unexpected callable {route}"),
        }
    }
}

fn fixture(
    advertised: bool,
    uncertain: bool,
) -> (FirebaseRegistry, ReceiverStatus, PrivateKeys, Arc<Calls>) {
    let keys = PrivateKeys::generate(&mut rand::rngs::OsRng);
    let value = json!({"receiver_revision":7,"receiver_installation_id":"machine",
        "receiver_key_id":keys.verifier_key_id("owner"),"receiver_machine_label":"Here",
        "receiving_enabled":true,"is_me":true,"receiver_features":if advertised { vec!["files.v1","receivers.friend.v1"] } else { vec![] }});
    let status = serde_json::from_value(value.clone()).unwrap();
    let calls = Arc::new(Calls {
        status: value,
        enrollments: Mutex::default(),
        uncertain,
    });
    let registry = FirebaseRegistry {
        calls: calls.clone(),
        uid: "owner".into(),
        installation_id: "machine".into(),
        machine_label: "Here".into(),
        enrollment: None,
    };
    (registry, status, keys, calls)
}

#[tokio::test]
async fn startup_advertises_files_once_with_the_current_revision_and_no_takeover() {
    let (mux, _) = MultiplexerState::new();
    let service = ServiceFixture::new(mux);
    let (registry, status, keys, calls) = fixture(false, false);
    *service.service.neighbor_cell().receiver.lock() = Some(status.clone());
    let (first, second) = tokio::join!(
        service
            .service
            .advertise_neighbor_files(&registry, &status, &keys, 0),
        service
            .service
            .advertise_neighbor_files(&registry, &status, &keys, 0),
    );
    assert_eq!(
        [first.unwrap(), second.unwrap()]
            .into_iter()
            .filter(|changed| *changed)
            .count(),
        1
    );
    assert!(service.service.neighbor_cell().receiver.lock().is_none());
    let data = calls.enrollments.lock().unwrap()[0].clone();
    assert_eq!(calls.enrollments.lock().unwrap().len(), 1);
    assert_eq!(data["installation_id"], "machine");
    assert_eq!(data["expected_receiver_revision"], 7);
    assert_eq!(data["takeover"], false);
    assert_eq!(data["features"], json!(["files.v1","receivers.friend.v1"]));
    assert_eq!(data["public_key_spki_b64"], keys.signing_spki());
    assert_eq!(
        data["encryption_public_key_spki_b64"],
        keys.encryption_spki()
    );
    registry
        .advertise_file_support(&status, &keys)
        .await
        .unwrap();
    assert_eq!(calls.enrollments.lock().unwrap()[1], data);
}

#[tokio::test]
async fn startup_skips_advertised_or_foreign_receivers_and_keeps_uncertainty_unconfirmed() {
    let (mux, _) = MultiplexerState::new();
    let service = ServiceFixture::new(mux);
    let (registry, status, keys, calls) = fixture(true, false);
    assert!(!service
        .service
        .advertise_neighbor_files(&registry, &status, &keys, 0)
        .await
        .unwrap());
    assert!(calls.enrollments.lock().unwrap().is_empty());

    let (registry, mut status, keys, calls) = fixture(false, false);
    status.is_me = false;
    assert_eq!(
        registry.advertise_file_support(&status, &keys).await,
        Err(Error::NotReceiver)
    );
    status.is_me = true;
    let other = PrivateKeys::generate(&mut rand::rngs::OsRng);
    assert_eq!(
        registry.advertise_file_support(&status, &other).await,
        Err(Error::InvalidKey)
    );
    status.receiver_revision += 1;
    assert_eq!(
        registry.advertise_file_support(&status, &keys).await,
        Err(Error::ReceiverRevisionStale)
    );
    assert!(calls.enrollments.lock().unwrap().is_empty());

    let (mux, _) = MultiplexerState::new();
    let service = ServiceFixture::new(mux);
    let (registry, status, keys, calls) = fixture(false, true);
    assert_eq!(
        service
            .service
            .advertise_neighbor_files(&registry, &status, &keys, 0)
            .await,
        Err(Error::Transport)
    );
    assert_eq!(
        *service
            .service
            .neighbor_cell()
            .file_advertised_generation
            .lock()
            .await,
        None
    );
    assert_eq!(calls.enrollments.lock().unwrap().len(), 1);
}
