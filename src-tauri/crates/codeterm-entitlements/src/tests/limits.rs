use super::support::{admin, binding, free, paid, spec, with, LocalSigner, KEY_ID, T};
use crate::payload::{Limits, COMPILED_FREE_LIMITS, TAB_TEAM_LIMITS_POLICY_VERSION};
use crate::policy::{Policy, TrustedKey};
use crate::verify::verify;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PACKET: &[u8] = include_bytes!("../../vectors/limits-vectors.json");
// The producer repository pins the same digest; a one-sided edit fails both suites.
const PACKET_SHA256: &str = "74a8a9ee66a9b27f96419018954332a59543b917673fb1a65d8e501809646ac1";

fn packet() -> Value {
    serde_json::from_slice(PACKET).expect("limits-vectors.json")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn policy(signer: &LocalSigner, policy_version: u64) -> Policy {
    let mut spec = spec(vec![TrustedKey {
        key_id: KEY_ID.into(),
        spki_der_base64: signer.spki(),
    }]);
    spec.policy_version = policy_version;
    Policy::new(spec).expect("policy")
}

fn base(authority: &str) -> Value {
    match authority {
        "free" => free(),
        "billing" => paid(),
        "admin" => admin(),
        other => panic!("authority {other}"),
    }
}

fn maxima(limits: &Limits) -> Value {
    json!({
        "mesh.machines": limits.mesh_machines.max,
        "workspaces": limits.workspaces.max,
        "mem.items": limits.mem_items.max,
        "teams": limits.teams.max,
        "agent.tabs": limits.agent_tabs.max,
    })
}

#[test]
fn limits_packet_is_pinned() {
    assert_eq!(hex(&Sha256::digest(PACKET)), PACKET_SHA256);
    assert_eq!(
        packet()["tab_team_limits_policy_version"],
        TAB_TEAM_LIMITS_POLICY_VERSION
    );
}

#[test]
fn compiled_free_baseline_matches_the_packet() {
    let compiled = &packet()["compiled_free"];
    assert_eq!(maxima(&COMPILED_FREE_LIMITS), {
        let mut expected = json!({});
        for (key, limit) in compiled.as_object().unwrap() {
            expected[key] = limit["max"].clone();
        }
        expected
    });
    assert_eq!(COMPILED_FREE_LIMITS.teams.max, Some(6));
    assert_eq!(COMPILED_FREE_LIMITS.agent_tabs.max, Some(35));
}

/// Every grant shape the producer may sign verifies, and both shapes resolve to the same catalog.
#[test]
fn consumer_accepts_old_and_new_limit_shapes() {
    let signer = LocalSigner::new();
    for case in packet()["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let version = case["policy_version"].as_u64().unwrap();
        let payload = with(
            base(case["authority"].as_str().unwrap()),
            json!({"policy_version": version, "limits": case["limits"]}),
        );
        let outcome = verify(
            &signer.sign(&payload),
            &policy(&signer, version),
            &binding(),
            None,
            T,
        );
        match &case["consumer"] {
            Value::String(verdict) if verdict == "invalid" => {
                assert!(outcome.is_err(), "{id} must be refused");
            }
            expected => {
                let accepted = outcome.unwrap_or_else(|error| panic!("{id}: {error:?}"));
                assert_eq!(&maxima(&accepted.verdict.limits), expected, "{id}");
            }
        }
    }
}
