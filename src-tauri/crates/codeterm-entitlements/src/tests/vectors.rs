use crate::jcs;
use crate::payload::{Authority, Enforcement, GrantPayload, Mode, PayloadDefect};
use crate::policy::{Policy, PolicySpec, TrustedKey};
use crate::verdict::{compiled_free, TimeState};
use crate::verify::{
    verify, Accepted, Binding, DetachedGrant, Facet, Facets, Refusal, Rejected, RevisionOrder,
    VerifiedGrant,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

const PACKET: &[u8] = include_bytes!("../../vectors/grant-vectors.json");
// The producer repository pins the same digest; a one-sided edit fails both suites.
const PACKET_SHA256: &str = "f18139a3a1870d7cccca6a9dad50d370575ace27606f6dcc69a0219935f98808";

fn packet() -> Value {
    serde_json::from_slice(PACKET).expect("grant-vectors.json")
}

fn state_name(state: TimeState) -> &'static str {
    match state {
        TimeState::NotYetValid => "not-yet-valid",
        TimeState::Fresh => "fresh",
        TimeState::Stale => "stale",
        TimeState::Expired => "expired",
    }
}

fn exact_payload(id: &str) -> Value {
    serde_json::from_str(vector(id)["exact_payload"]["utf8"].as_str().unwrap()).unwrap()
}

fn vector(id: &str) -> Value {
    packet()["vectors"]
        .as_array()
        .expect("vectors")
        .iter()
        .find(|v| v["id"] == id)
        .cloned()
        .expect(id)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn policy_for(context: &Value) -> Policy {
    let key = &packet()["key"];
    Policy::new(PolicySpec {
        project: context["project"].as_str().expect("project").into(),
        mode: if context["env"] == "test" {
            Mode::Test
        } else {
            Mode::Live
        },
        policy_version: context["policy_version"].as_u64().expect("policy_version"),
        policy_epoch: context["current_policy_epoch"]
            .as_str()
            .expect("epoch")
            .into(),
        enforcement: Enforcement {
            enabled: false,
            rollback: false,
        },
        keys: vec![TrustedKey {
            key_id: key["key_id"].as_str().expect("key_id").into(),
            spki_der_base64: key["public_key_spki_base64"].as_str().expect("spki").into(),
        }],
    })
    .expect("fixture policy")
}

/// A cached grant at `revision` in the paid vector's context, standing in for an earlier verification.
fn cached_at(revision: u64) -> VerifiedGrant {
    let mut value = exact_payload("valid-paid");
    value["revision"] = revision.into();
    let bytes = jcs::encode(&value).unwrap().into_bytes();
    let payload: GrantPayload = serde_json::from_value(value).unwrap();
    VerifiedGrant {
        payload,
        bytes,
        key_id: "e5-rfc8032-test-only".into(),
    }
}

fn run(id: &str, cached: Option<&VerifiedGrant>) -> Result<Accepted, Rejected> {
    let v = vector(id);
    let context = &v["caller_context"];
    let grant: DetachedGrant =
        serde_json::from_value(v["response"]["grant"].clone()).expect("grant");
    let binding = Binding {
        uid: context["uid"].as_str().unwrap(),
        installation_id: context["installation_id"].as_str().unwrap(),
    };
    verify(
        &grant,
        &policy_for(context),
        &binding,
        cached,
        context["now"].as_u64().unwrap(),
    )
}

fn run_vector(id: &str) -> Result<Accepted, Rejected> {
    let cached = vector(id)["caller_context"]["cached_revision"]
        .as_u64()
        .unwrap();
    let cached = (cached > 0).then(|| cached_at(cached));
    run(id, cached.as_ref())
}

fn facets(outcome: &Result<Accepted, Rejected>) -> Facets {
    match outcome {
        Ok(accepted) => accepted.facets,
        Err(rejected) => rejected.facets,
    }
}

/// Evaluated facets must agree with the packet; short-circuited ones stay `NotEvaluated`.
fn assert_matches_expected(id: &str, outcome: &Result<Accepted, Rejected>) {
    let expected = &vector(id)["expected"];
    let facets = facets(outcome);
    let agrees = |facet: Facet, key: &str| match facet {
        Facet::NotEvaluated => true,
        Facet::Passed => expected[key] == true,
        Facet::Failed => expected[key] == false,
    };
    assert!(
        agrees(facets.trusted_key, "trusted_key"),
        "{id} trusted_key"
    );
    assert!(
        agrees(facets.mathematical_signature, "mathematical_signature"),
        "{id} signature"
    );
    assert!(
        agrees(facets.context_matches, "context_matches"),
        "{id} context"
    );
    assert!(
        agrees(facets.revision_newer(), "revision_newer"),
        "{id} revision"
    );
    if let Some(state) = facets.time_state {
        let name = match state {
            TimeState::Fresh | TimeState::Stale => "lease",
            other => state_name(other),
        };
        assert_eq!(expected["time_state"], name, "{id} time_state");
    }
}

#[test]
fn packet_sha256_is_pinned() {
    assert_eq!(hex(&Sha256::digest(PACKET)), PACKET_SHA256);
}

#[test]
fn grant_payloads_are_canonical_producer_bytes() {
    for v in packet()["vectors"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        let exact = &v["exact_payload"];
        let decoded = URL_SAFE_NO_PAD
            .decode(v["response"]["grant"]["payload"].as_str().unwrap())
            .unwrap();
        assert_eq!(hex(&decoded), exact["hex"].as_str().unwrap(), "{id} hex");
        assert_eq!(
            decoded.len() as u64,
            exact["byte_length"].as_u64().unwrap(),
            "{id} length"
        );
        assert_eq!(
            hex(&Sha256::digest(&decoded)),
            exact["sha256"].as_str().unwrap(),
            "{id} sha256"
        );
        let utf8 = exact["utf8"].as_str().unwrap();
        assert_eq!(utf8.as_bytes(), decoded.as_slice(), "{id} utf8");
        let reencoded = jcs::encode(&serde_json::from_str(utf8).unwrap()).unwrap();
        assert_eq!(reencoded, utf8, "{id} jcs");
    }
}

#[test]
fn every_vector_matches_its_expected_outcome() {
    for v in packet()["vectors"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        let outcome = run_vector(id);
        assert_matches_expected(id, &outcome);
        let refusal = outcome.as_ref().err().map(|rejected| rejected.refusal);
        let want = match id {
            "wrong-installation" => Some(Refusal::Installation),
            "older-policy-epoch" => Some(Refusal::PolicyEpoch),
            "tampered-payload" => Some(Refusal::Signature),
            "unknown-key-id" => Some(Refusal::UnknownKey),
            _ => None,
        };
        assert_eq!(refusal, want, "{id}");
    }
}

#[test]
fn accepted_vectors_carry_their_verdicts() {
    let paid = run_vector("valid-paid").unwrap();
    assert_eq!(
        (paid.verdict.state, paid.verdict.authority),
        (TimeState::Fresh, Authority::Billing)
    );
    assert_eq!(paid.facets.revision, Some(RevisionOrder::Higher));
    let admin = run_vector("valid-admin").unwrap();
    assert_eq!(
        (admin.verdict.authority, admin.facets.revision),
        (Authority::Admin, Some(RevisionOrder::First))
    );
    assert!(admin.verdict.perks.priority_support);
    let expired = run_vector("expired-lease").unwrap();
    assert_eq!(expired.verdict.state, TimeState::Expired);
    assert_eq!(expired.verdict.effective(), compiled_free());
    let ended = run_vector("period-ended").unwrap();
    assert_eq!(ended.verdict.state, TimeState::Expired);
    assert_eq!(ended.verdict.effective(), compiled_free());
    assert!(ended.grant.payload().period_end < Some(ended.grant.payload().expires_at));
    for accepted in [&paid, &admin, &expired, &ended] {
        assert_eq!(accepted.verdict.transition_reason, None);
    }
}

#[test]
fn producer_windows_match_consumer_time_states() {
    for window in packet()["windows"].as_array().unwrap() {
        let id = window["vector"].as_str().unwrap();
        let grant = run_vector(id).unwrap().grant;
        for checkpoint in window["checkpoints"].as_array().unwrap() {
            let now = checkpoint["now"].as_u64().unwrap();
            let expected = if checkpoint["state"] == "not-yet-valid"
                && now
                    >= grant
                        .payload()
                        .not_before
                        .saturating_sub(crate::verify::MAX_CLOCK_SKEW_SECS)
            {
                "fresh"
            } else {
                checkpoint["state"].as_str().unwrap()
            };
            assert_eq!(
                state_name(grant.verdict(now).state),
                expected,
                "{id} now={now}"
            );
        }
    }
}

#[test]
fn producer_refusals_fail_consumer_invariants() {
    for refusal in packet()["refusals"].as_array().unwrap() {
        let id = refusal["id"].as_str().unwrap();
        let mut value = exact_payload(refusal["base"].as_str().unwrap());
        super::support::patch(&mut value, refusal["set"].clone());
        let payload: GrantPayload = serde_json::from_value(value).unwrap();
        let want = match refusal["defect"].as_str().unwrap() {
            "time" => PayloadDefect::Time,
            "expiry" => PayloadDefect::Expiry,
            "grace" => PayloadDefect::Grace,
            other => panic!("{id}: defect {other}"),
        };
        assert_eq!(payload.check_invariants(), Err(want), "{id}");
    }
}

#[test]
fn signed_free_downgrade_replaces_cached_paid() {
    let paid = run_vector("valid-paid").unwrap().grant;
    let free = run("signed-free-downgrade", Some(&paid)).unwrap();
    assert_eq!(free.facets.revision, Some(RevisionOrder::Higher));
    assert_eq!(free.verdict.authority, Authority::Free);
    assert!(!free.verdict.enforcement.rollback);
    let mut policy_one_free = compiled_free();
    policy_one_free.limits.mem_items.max = Some(1000);
    assert_eq!(free.verdict.effective(), policy_one_free);
    let stale = run("valid-paid", Some(&free.grant)).unwrap_err();
    assert_eq!(stale.refusal, Refusal::Revision);
    assert_eq!(stale.facets.revision, Some(RevisionOrder::Lower));
}

#[test]
fn equal_revision_reverifies() {
    let paid = run_vector("valid-paid").unwrap().grant;
    let again = run("valid-paid", Some(&paid)).unwrap();
    assert_eq!(again.facets.revision, Some(RevisionOrder::Equal));
    assert_eq!(again.grant, paid);
}

#[test]
fn refusals_short_circuit_later_facets() {
    let unknown = run_vector("unknown-key-id").unwrap_err();
    assert_eq!(
        unknown.facets,
        Facets {
            trusted_key: Facet::Failed,
            ..Facets::default()
        }
    );
    let tampered = run_vector("tampered-payload").unwrap_err();
    assert_eq!(
        tampered.facets,
        Facets {
            trusted_key: Facet::Passed,
            mathematical_signature: Facet::Failed,
            ..Facets::default()
        }
    );
    let wrong = run_vector("wrong-installation").unwrap_err();
    assert_eq!(
        (wrong.facets.canonical_payload, wrong.facets.context_matches),
        (Facet::Passed, Facet::Failed)
    );
    assert_eq!(
        (wrong.facets.time_state, wrong.facets.revision),
        (None, None)
    );
}

#[test]
fn verify_stays_bounded_in_the_unoptimised_profile() {
    let v = vector("valid-paid");
    let context = &v["caller_context"];
    let grant: DetachedGrant = serde_json::from_value(v["response"]["grant"].clone()).unwrap();
    let policy = policy_for(context);
    let binding = Binding {
        uid: "fixture-user",
        installation_id: "fixture-installation",
    };
    let started = std::time::Instant::now();
    verify(&grant, &policy, &binding, None, 1_700_000_060).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(25),
        "verify took {elapsed:?}"
    );
}
