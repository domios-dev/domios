use super::support::session;
use crate::account_mesh::callable::*;
use crate::account_mesh::wire::*;
use crate::account_mesh::MeshRegistry;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Mutex;

const PACKET: &str = include_str!("account-mesh-packet.json");

#[tokio::test]
async fn prior_session_revoke_uses_the_confirmed_callable_contract_and_typed_refusals() {
    let registry = registry(Ok(
        json!({"result":{"schema_version":1,"revoked_count":1,"receiver_disabled":true}}),
    ));
    let target = PriorSessionsTarget {
        installation_id: "desk-7f3a".into(),
        cutoff: 123,
    };
    let result = registry
        .revoke_prior_sessions(&session("user-1"), &target)
        .await
        .unwrap();
    assert!(result.receiver_disabled);
    assert_eq!(
        registry.transport_calls(),
        vec![(
            "revokePriorInstallationSessions".into(),
            "token-user-1".into(),
            json!({"installation_id":"desk-7f3a","cutoff":123})
        )]
    );
    for (reason, expected) in [
        (
            "installation_unverified",
            RegistryRefusal::InstallationUnverified,
        ),
        (
            "cutoff_after_current_session",
            RegistryRefusal::CutoffAfterCurrentSession,
        ),
    ] {
        assert_eq!(
            RegistryRefusal::from_details(&json!({"reason":reason})),
            expected
        );
    }
}

#[tokio::test]
async fn entitlements_challenge_uses_the_online_route_and_exact_body() {
    let registry = registry(Ok(json!({"result":{"nonce":"fixture-nonce"}})));
    let request = ChallengeRequest {
        endpoint: ProofEndpoint::GetEntitlements,
        key_id: "1".repeat(64),
        body: json!({"installation_id":"desk-7f3a"}),
    };
    assert_eq!(
        registry
            .challenge(&session("user-1"), &request)
            .await
            .unwrap(),
        "fixture-nonce"
    );
    assert_eq!(
        registry.transport_calls(),
        vec![(
            "issueInstallationChallenge".into(),
            "token-user-1".into(),
            json!({"installation_id":"desk-7f3a", "key_id":"1".repeat(64), "endpoint":"getEntitlements", "body":request.body})
        )]
    );
}

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

#[derive(Default)]
struct Revocations(Mutex<Vec<String>>);
#[async_trait]
impl crate::session_revocation::SessionRevocationObserver for Revocations {
    async fn revoked(&self, session: &crate::Session) {
        self.0.lock().unwrap().push(session.identity.uid.clone());
    }
}

#[tokio::test]
async fn session_revoked_from_every_mesh_callable_notifies_the_session_owner() {
    let observer = std::sync::Arc::new(Revocations::default());
    let body = json!({"error":{"status":"UNAUTHENTICATED","details":{"reason":"session_revoked"}}});
    let registry = registry(Err(CallableFailure::Status {
        status: 401,
        retry_after_seconds: None,
        body: Some(body),
    }))
    .with_revocations(observer.clone());
    let session = session("user-1");
    let proof = InstallationProof {
        schema_version: 1,
        key_id: "k".into(),
        generation: 1,
        timestamp: 123,
        nonce: "n".into(),
        signature: "s".into(),
    };
    assert_eq!(
        registry
            .challenge(
                &session,
                &ChallengeRequest {
                    endpoint: ProofEndpoint::MeshAdmission,
                    key_id: "k".into(),
                    body: json!({})
                }
            )
            .await,
        Err(RegistryRefusal::Unauthenticated)
    );
    assert_eq!(
        registry
            .enroll_key(
                &session,
                &InstallationEnrollment {
                    body: InstallationKeyBody {
                        installation_id: "caller".into(),
                        public_key_spki_base64: "key".into(),
                        operation: KeyOperation::Enroll,
                        generation_policy: None,
                        expected_generation: 0
                    },
                    proof: proof.clone(),
                }
            )
            .await,
        Err(RegistryRefusal::Unauthenticated)
    );
    assert_eq!(
        registry
            .admission(
                &session,
                &SignedEnvelope {
                    request: MeshAdmissionBody {
                        schema_version: 1,
                        installation_id: "caller".into(),
                        request_id: "a".repeat(64),
                        requested_membership_seconds: None,
                        operation: MeshOperation::SessionRevoke {
                            target: SessionTarget {
                                installation_id: "target".into()
                            }
                        }
                    },
                    installation_proof: proof,
                }
            )
            .await,
        Err(RegistryRefusal::Unauthenticated)
    );
    assert_eq!(*observer.0.lock().unwrap(), ["user-1", "user-1", "user-1"]);
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
            retry_after_seconds: None,
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
    let bare = |status| CallableFailure::Status {
        status,
        body: None,
        retry_after_seconds: None,
    };
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
        requested_membership_seconds: None,
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

#[test]
fn http_status_body_matrix_preserves_permanent_auth_transient_and_throttle_classes() {
    let bodies = [
        None,
        Some(json!({"error":{"status":"UNKNOWN"}})),
        Some(json!({"error":{"details":{"reason":"rate_limited","retry_after_seconds":45}}})),
        Some(json!({"error":{"details":{"reason":"unknown"}}})),
    ];
    for body in bodies {
        for (status, expected) in [
            (401, RegistryRefusal::Unauthenticated),
            (403, RegistryRefusal::Unavailable),
            (404, RegistryRefusal::Unavailable),
            (500, RegistryRefusal::Unavailable),
            (503, RegistryRefusal::Unavailable),
        ] {
            let error = CallableFailure::Status {
                status,
                body: body.clone(),
                retry_after_seconds: Some(60),
            };
            assert_eq!(refusal(&error), expected, "{status} {body:?}");
        }
        let error = CallableFailure::Status {
            status: 429,
            body: body.clone(),
            retry_after_seconds: Some(60),
        };
        assert_eq!(error.retry_after_seconds(), Some(60));
        assert_eq!(
            refusal(&error),
            RegistryRefusal::RateLimited {
                retry_after_seconds: 60
            }
        );
    }
    assert_eq!(
        refusal(&CallableFailure::Status {
            status: 429,
            body: None,
            retry_after_seconds: None
        }),
        RegistryRefusal::RateLimited {
            retry_after_seconds: 1
        }
    );
}
