use super::*;
use crate::tests::{activation, paid_grant_response, MemoryGrants};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::Notify;

mod mesh;

struct RekeyProof(AtomicU64);

#[async_trait]
impl InstallationProofSource for RekeyProof {
    async fn proof(&self, session: &Session, id: &InstallationId) -> Result<InstallationProof> {
        let mut proof = FixtureProof.proof(session, id).await?;
        proof.generation = self.0.load(Ordering::SeqCst);
        proof.key_id = format!("{:064x}", proof.generation);
        Ok(proof)
    }

    async fn replacement_proof(
        &self,
        session: &Session,
        id: &InstallationId,
        generation: u64,
    ) -> Option<InstallationProof> {
        self.proof(session, id)
            .await
            .ok()
            .filter(|proof| proof.generation > generation)
    }
}

struct RekeyTransport {
    requests: parking_lot::Mutex<Vec<Option<InstallationProof>>>,
    received_first: Notify,
    release_first: Notify,
    first_status: &'static str,
    first_detail: Option<&'static str>,
    refuse_retry: bool,
}

impl RekeyTransport {
    fn new(
        first_status: &'static str,
        first_detail: Option<&'static str>,
        refuse_retry: bool,
    ) -> Self {
        Self {
            requests: Default::default(),
            received_first: Notify::new(),
            release_first: Notify::new(),
            first_status,
            first_detail,
            refuse_retry,
        }
    }
}

#[async_trait]
impl CloudTransport for RekeyTransport {
    async fn execute(&self, request: reqwest::Request) -> Result<(bool, serde_json::Value)> {
        let body: serde_json::Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        let proof: Option<InstallationProof> = body
            .pointer("/data/installation_proof")
            .map(|value| serde_json::from_value(value.clone()).unwrap());
        let generation = proof.as_ref().map(|proof| proof.generation);
        self.requests.lock().push(proof);
        match generation {
            Some(3) => {
                self.received_first.notify_one();
                self.release_first.notified().await;
                let mut error = serde_json::json!({"status":self.first_status});
                if let Some(detail) = self.first_detail {
                    error["details"] = serde_json::json!({"detail":detail});
                }
                Ok((false, serde_json::json!({"error":error})))
            }
            Some(4) if self.refuse_retry => Ok((
                false,
                serde_json::json!({"error":{"status":"FAILED_PRECONDITION"}}),
            )),
            Some(4) => {
                let envelope: serde_json::Value =
                    serde_json::from_str(paid_grant_response().get()).unwrap();
                Ok((true, serde_json::json!({"result":envelope})))
            }
            None => Ok((
                true,
                serde_json::json!({"result":serde_json::from_str::<serde_json::Value>(include_str!(
                    "../../../../fixtures/unsigned_schema2_response.json"
                )).unwrap()}),
            )),
            other => panic!("unexpected proof generation {other:?}"),
        }
    }
}

async fn first_settled_read(
    status: &'static str,
    detail: Option<&'static str>,
    rotate: bool,
    refuse_retry: bool,
) -> (Snapshot, Vec<Option<u64>>) {
    let source = Arc::new(RekeyProof(AtomicU64::new(3)));
    let transport = Arc::new(RekeyTransport::new(status, detail, refuse_retry));
    let billing = fixture_billing(source.clone(), transport.clone());
    let session = fixture_session();
    let (snapshot, ()) = tokio::join!(
        billing.refresh(
            Some(&session),
            RefreshEvent::SignIn,
            std::time::Instant::now()
        ),
        async {
            transport.received_first.notified().await;
            if rotate {
                source.0.store(4, Ordering::SeqCst);
            }
            transport.release_first.notify_one();
        }
    );
    let requests = transport
        .requests
        .lock()
        .iter()
        .map(|proof| proof.as_ref().map(|proof| proof.generation))
        .collect();
    (snapshot, requests)
}

fn fixture_billing(
    source: Arc<dyn InstallationProofSource>,
    transport: Arc<RekeyTransport>,
) -> Billing {
    let mut cloud = Cloud::fixture("https://fixture.test/", id()).with_installation_proofs(source);
    cloud.transport = transport;
    Billing::new(Arc::new(cloud)).with_test_grants(
        activation("fixture-online-v1"),
        id().as_str(),
        Arc::new(MemoryGrants::default()),
        Arc::new(|| 1_700_000_060),
    )
}

fn fixture_session() -> Session {
    let mut session = local_session();
    session.identity.uid = "fixture-user".into();
    session.login_generation = 17;
    session
}

#[tokio::test]
async fn unrelated_grant_failure_falls_back_without_signed_retry() {
    for (status, detail, rotate) in [
        ("FAILED_PRECONDITION", None, false),
        ("FAILED_PRECONDITION", Some("signer_failed"), true),
        ("PERMISSION_DENIED", None, true),
    ] {
        let (snapshot, requests) = first_settled_read(status, detail, rotate, false).await;
        assert_eq!(requests, [Some(3), None]);
        assert!(!snapshot.checking);
        assert_eq!(snapshot.verification, Some(Verification::Unconfirmed));
        assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
    }
}

#[tokio::test]
async fn failed_replacement_grant_falls_back_after_one_signed_retry() {
    let (snapshot, requests) = first_settled_read("FAILED_PRECONDITION", None, true, true).await;
    assert_eq!(requests, [Some(3), Some(4), None]);
    assert_eq!(snapshot.verification, Some(Verification::Unconfirmed));
    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
}
