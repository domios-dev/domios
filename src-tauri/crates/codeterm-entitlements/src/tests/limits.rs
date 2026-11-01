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
    let mut expected = {
        let mut expected = json!({});
        for (key, limit) in compiled.as_object().unwrap() {
            expected[key] = limit["max"].clone();
        }
        expected
    };
    expected["mem.items"] = json!(500);
    assert_eq!(maxima(&COMPILED_FREE_LIMITS), expected);
    assert_eq!(crate::activation::PRODUCTION.policy_version, 2);
    assert_eq!(crate::activation::TEST.policy_version, 2);
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
        if version >= 2 && (case["authority"] == "free" || id == "old-shape-under-new-policy") {
            assert!(
                outcome.is_err(),
                "archived Free baseline must be refused: {id}"
            );
            continue;
        }
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

#[test]
fn policy_two_free_uses_the_new_baseline_and_requires_all_five_limits() {
    let mut payload = free();
    payload["policy_version"] = json!(2);
    payload["limits"]["mem.items"]["max"] = json!(500);
    payload["limits"]["teams"] = json!({"max": 6, "scope": "installation"});
    payload["limits"]["agent.tabs"] = json!({"max": 35, "scope": "installation"});
    let parsed: crate::payload::GrantPayload = serde_json::from_value(payload.clone()).unwrap();
    parsed.check_invariants().expect("current Free payload");
    assert_eq!(parsed.effective_limits(), COMPILED_FREE_LIMITS);
    for key in ["teams", "agent.tabs"] {
        let mut missing = payload.clone();
        missing["limits"].as_object_mut().unwrap().remove(key);
        let parsed: crate::payload::GrantPayload = serde_json::from_value(missing).unwrap();
        assert!(parsed.check_invariants().is_err());
    }
}

#[test]
fn future_paid_tier_preserves_its_label_and_signed_limits() {
    let mut payload = paid();
    payload["policy_version"] = json!(2);
    payload["label"] = json!("future-tier");
    payload["limits"]["workspaces"]["max"] = json!(19);
    payload["limits"]["mem.items"]["max"] = json!(4321);
    payload["limits"]["teams"] = json!({"max": 8, "scope": "installation"});
    payload["limits"]["agent.tabs"] = json!({"max": 64, "scope": "installation"});
    let parsed: crate::payload::GrantPayload = serde_json::from_value(payload).unwrap();
    parsed.check_invariants().expect("future tier payload");
    let verdict = crate::verdict::Verdict::of(&parsed, T);
    assert_eq!(verdict.label.as_str(), "future-tier");
    let access = verdict.effective();
    assert_eq!(access.limits.workspaces.max, Some(19));
    assert_eq!(access.limits.mem_items.max, Some(4321));
    assert_eq!(access.limits.teams.max, Some(8));
    assert_eq!(access.limits.agent_tabs.max, Some(64));
    let gate = crate::gate::LimitGate::new(access, true);
    for (key, max) in [
        (crate::gate::LimitKey::Workspaces, 19),
        (crate::gate::LimitKey::Teams, 8),
        (crate::gate::LimitKey::AgentTabs, 64),
    ] {
        assert!(gate.admit(key, max - 1).is_ok());
        assert_eq!(gate.admit(key, max).unwrap_err().max, max as u64);
    }
}
