use super::support::session;
use crate::account_mesh::callable::*;
use crate::account_mesh::wire::*;
use crate::account_mesh::MeshRegistry;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Mutex;

const PACKET: &str = include_str!("account-mesh-packet.json");

struct Scripted {
    reply: Result<Value, CallableFailure>,
    seen: Mutex<Vec<(String, String, Value)>>,
}

#[async_trait]
impl CallableTransport for Scripted {
    async fn call(
        &self,
        route: &str,
        id_token: &str,
        data: Value,
    ) -> Result<Value, CallableFailure> {
        self.seen
            .lock()
            .unwrap()
            .push((route.into(), id_token.into(), data));
        self.reply.clone()
    }
}

fn registry(reply: Result<Value, CallableFailure>) -> CallableRegistry<Scripted> {
    CallableRegistry::new(Scripted {
        reply,
        seen: Mutex::new(Vec::new()),
    })
}

fn http(code: &str) -> u16 {
    match code {
        "invalid-argument" => 400,
        "unauthenticated" => 401,
        "permission-denied" => 403,
        "aborted" => 409,
        "resource-exhausted" => 429,
        "failed-precondition" => 400,
        _ => 503,
    }
}

#[test]
fn account_mesh_callable_errors_keep_the_typed_server_refusal() {
    let packet: Value = serde_json::from_str(PACKET).unwrap();
    for (name, refusal) in packet["refusals"].as_object().unwrap() {
        let failure = CallableFailure::Status {
            status: http(refusal["code"].as_str().unwrap()),
            body: Some(
                json!({"error": {"status": refusal["code"], "details": refusal["details"]}}),
            ),
        };
        let mapped =
            super::super::AccountMeshRefusal::from(super::super::callable::refusal(&failure));
        assert_eq!(
            serde_json::to_value(mapped).unwrap(),
            refusal["details"],
            "{name}"
        );
    }
    assert_eq!(refusal(&CallableFailure::Offline), RegistryRefusal::Offline);
    let bare = |status| CallableFailure::Status { status, body: None };
    assert_eq!(refusal(&bare(401)), RegistryRefusal::Unauthenticated);
    assert_eq!(
        refusal(&bare(400)),
        RegistryRefusal::Invalid,
        "no details is never mesh.machines"
    );
    assert_eq!(refusal(&bare(500)), RegistryRefusal::Unavailable);
}

#[tokio::test]
async fn account_mesh_callable_sends_the_packet_envelope_and_reads_the_result() {
    let packet: Value = serde_json::from_str(PACKET).unwrap();
    let published = packet["results"]["published"].clone();
    let registry = registry(Ok(json!({ "result": published })));
    let request: MeshAdmissionBody = MeshAdmissionBody {
        schema_version: 1,
        installation_id: "desk-7f3a".into(),
        request_id: format!("{}1", "0".repeat(63)),
        operation: MeshOperation::Publish {
            expected_metadata_revision: 0,
            endpoint: MeshEndpoint::parse("100.64.0.7", 7685).unwrap().wire(),
        },
    };
    let proof = InstallationProof {
        schema_version: 1,
        key_id: "1".repeat(64),
        generation: 1,
        timestamp: 1_700_000_000,
        nonce: "n".into(),
        signature: "s".repeat(86),
    };
    let envelope = SignedEnvelope {
        request,
        installation_proof: proof,
    };
    let result = registry
        .admission(&session("user-1"), &envelope)
        .await
        .unwrap();
    assert!(matches!(result, MeshResult::Published(_)));
    let seen = registry.transport_calls();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, ROUTE_ADMISSION);
    assert_eq!(seen[0].1, "token-user-1");
    assert_eq!(seen[0].2["request"], packet["requests"]["publish"]);
    assert_eq!(seen[0].2["installation_proof"]["signature"], "s".repeat(86));
}

#[tokio::test]
async fn account_mesh_callable_malformed_success_is_an_outage() {
    let challenge = ChallengeRequest {
        endpoint: ProofEndpoint::MeshAdmission,
        key_id: "1".repeat(64),
        body: json!({}),
    };
    for body in [
        json!({}),
        json!({"result": {"nonce": 7}}),
        json!([1]),
        json!({"result": {"nonce": "x", "extra": 1}}),
    ] {
        let registry = registry(Ok(body.clone()));
        assert_eq!(
            registry.challenge(&session("user-1"), &challenge).await,
            Err(RegistryRefusal::Unavailable),
            "{body}"
        );
    }
    let registry = registry(Ok(json!({"result": {"nonce": "n.a.b"}})));
    assert_eq!(
        registry.challenge(&session("user-1"), &challenge).await,
        Ok("n.a.b".into())
    );
}

impl CallableRegistry<Scripted> {
    fn transport_calls(&self) -> Vec<(String, String, Value)> {
        self.transport().seen.lock().unwrap().clone()
    }
}
