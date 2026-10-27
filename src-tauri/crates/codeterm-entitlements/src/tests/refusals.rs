use super::support::{admin, binding, free, paid, with, LocalSigner, T};
use crate::payload::{PayloadDefect, TransitionReason};
use crate::verify::{verify, Binding, Facet, Refusal};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};

const NOW: u64 = T + 60;

fn refusal(signer: &LocalSigner, payload: &Value) -> Refusal {
    signer.check(payload, None, NOW).unwrap_err().refusal
}

#[test]
fn local_grants_verify() {
    let signer = LocalSigner::new();
    for payload in [paid(), admin(), free()] {
        signer.check(&payload, None, NOW).unwrap();
    }
}

#[test]
fn padding_and_alphabet_are_refused_before_signature() {
    let signer = LocalSigner::new();
    let policy = signer.policy();
    let good = signer.sign(&paid());
    let mut padded = good.clone();
    padded.payload.push('=');
    let mut standard = good.clone();
    standard.payload = format!("+/{}", &good.payload[2..]);
    for grant in [padded, standard] {
        let rejected = verify(&grant, &policy, &binding(), None, NOW).unwrap_err();
        assert_eq!(rejected.refusal, Refusal::PayloadEncoding);
        assert_eq!(rejected.facets.mathematical_signature, Facet::NotEvaluated);
    }
    let mut padded_signature = good.clone();
    padded_signature.signature.push_str("==");
    let rejected = verify(&padded_signature, &policy, &binding(), None, NOW).unwrap_err();
    assert_eq!(rejected.refusal, Refusal::SignatureEncoding);
}

#[test]
fn signature_length_is_measured_on_decoded_bytes() {
    let signer = LocalSigner::new();
    let policy = signer.policy();
    for length in [63usize, 65] {
        let mut grant = signer.sign(&paid());
        grant.signature = URL_SAFE_NO_PAD.encode(vec![7u8; length]);
        assert_eq!(
            verify(&grant, &policy, &binding(), None, NOW)
                .unwrap_err()
                .refusal,
            Refusal::SignatureLength
        );
    }
}

#[test]
fn algorithm_and_key_dispatch_come_first() {
    let signer = LocalSigner::new();
    let policy = signer.policy();
    let mut grant = signer.sign(&paid());
    grant.alg = "EdDSA".into();
    assert_eq!(
        verify(&grant, &policy, &binding(), None, NOW)
            .unwrap_err()
            .refusal,
        Refusal::Algorithm
    );
    let mut grant = signer.sign(&paid());
    grant.key_id = "other".into();
    grant.payload = "not base64 !".into();
    let rejected = verify(&grant, &policy, &binding(), None, NOW).unwrap_err();
    assert_eq!(rejected.refusal, Refusal::UnknownKey);
}

#[test]
fn signed_but_non_canonical_is_refused() {
    let signer = LocalSigner::new();
    let policy = signer.policy();
    let canonical = crate::jcs::encode(&paid()).unwrap();
    let spaced = canonical.replacen(':', ": ", 1);
    let duplicated = canonical.replacen("{\"aud\"", "{\"aud\":1,\"aud\"", 1);
    for bytes in [spaced, duplicated] {
        let rejected = verify(
            &signer.sign_bytes(bytes.as_bytes()),
            &policy,
            &binding(),
            None,
            NOW,
        )
        .unwrap_err();
        assert_eq!(rejected.refusal, Refusal::NonCanonical);
        assert_eq!(rejected.facets.mathematical_signature, Facet::Passed);
    }
    let invalid = verify(
        &signer.sign_bytes(b"{\"a\":"),
        &policy,
        &binding(),
        None,
        NOW,
    )
    .unwrap_err();
    assert_eq!(invalid.refusal, Refusal::Malformed);
}

#[test]
fn oversize_payload_is_refused() {
    let signer = LocalSigner::new();
    let names: Vec<String> = (0..64).map(|i| format!("{i:0>128}")).collect();
    assert_eq!(
        refusal(&signer, &with(paid(), json!({"capabilities": names}))),
        Refusal::PayloadSize
    );
}

#[test]
fn schema_is_exact() {
    let signer = LocalSigner::new();
    let mut missing = paid();
    missing.as_object_mut().unwrap().remove("period_end");
    let mut missing_max = paid();
    missing_max["limits"]["workspaces"]
        .as_object_mut()
        .unwrap()
        .remove("max");
    let cases = [
        with(paid(), json!({"extra": 1})),
        with(paid(), json!({"transition_reason": null})),
        with(paid(), json!({"transition_reason": "expired"})),
        with(paid(), json!({"revision": 1.5})),
        with(paid(), json!({"revision": -1})),
        with(
            paid(),
            json!({"aud": {"project": "demo-local", "env": "test", "x": 1}}),
        ),
        missing,
        missing_max,
    ];
    for payload in cases {
        assert_eq!(refusal(&signer, &payload), Refusal::Schema, "{payload}");
    }
}

#[test]
fn transition_reason_is_surfaced_never_inferred() {
    let signer = LocalSigner::new();
    let carried = signer
        .check(
            &with(free(), json!({"transition_reason": "revoked"})),
            None,
            NOW,
        )
        .unwrap();
    assert_eq!(
        carried.verdict.transition_reason,
        Some(TransitionReason::Revoked)
    );
    let absent = signer.check(&free(), None, T + 2_000_000).unwrap();
    assert_eq!(absent.verdict.transition_reason, None);
}

#[test]
fn producer_invariants_are_enforced() {
    use PayloadDefect as D;
    let signer = LocalSigner::new();
    let many: Vec<String> = (0..65).map(|i| format!("c{i}")).collect();
    let cases = [
        (with(paid(), json!({"schema_version": 2})), D::SchemaVersion),
        (with(paid(), json!({"iss": "other"})), D::Issuer),
        (with(paid(), json!({"sub": "a/b"})), D::Subject),
        (with(paid(), json!({"inst": ".."})), D::Installation),
        (with(paid(), json!({"inst": "a\\b"})), D::Installation),
        (with(paid(), json!({"revision": 0})), D::Revision),
        (
            with(paid(), json!({"revision": 9_007_199_254_740_992u64})),
            D::Revision,
        ),
        (with(paid(), json!({"policy_epoch": ""})), D::PolicyEpoch),
        (with(paid(), json!({"not_before": T + 1})), D::Time),
        (with(paid(), json!({"refresh_after": T + 3_601})), D::Time),
        (
            with(
                paid(),
                json!({"refresh_after": T + 86_400, "expires_at": T + 604_800, "grace_until": T + 1_209_600}),
            ),
            D::Time,
        ),
        (with(paid(), json!({"period_end": T})), D::PeriodEnd),
        (with(paid(), json!({"label": "admin"})), D::Label),
        (with(admin(), json!({"period_end": T + 10})), D::PeriodEnd),
        (
            with(
                paid(),
                json!({"expires_at": T + 86_401, "grace_until": T + 86_401}),
            ),
            D::Expiry,
        ),
        (
            with(
                paid(),
                json!({"period_end": T + 900, "expires_at": T + 900, "grace_until": T + 900}),
            ),
            D::Expiry,
        ),
        (with(paid(), json!({"grace_until": T + 86_401})), D::Grace),
        (with(admin(), json!({"grace_until": T + 86_401})), D::Grace),
        (
            with(
                paid(),
                json!({"limits": {"mesh.machines": {"max": 1, "scope": "installation"},
            "workspaces": {"max": 1, "scope": "installation"}, "mem.items": {"max": 1, "scope": "installation"}}}),
            ),
            D::LimitScope,
        ),
        (with(paid(), json!({"capabilities": many})), D::Capabilities),
        (
            with(paid(), json!({"capabilities": ["a", "a"]})),
            D::Capabilities,
        ),
        (
            with(paid(), json!({"capabilities": ["x".repeat(129)]})),
            D::Capabilities,
        ),
        (
            with(paid(), json!({"capabilities": ["tab\t"]})),
            D::Capabilities,
        ),
        (
            with(
                free(),
                json!({"perks": {"sponsor": true, "priority_support": false}}),
            ),
            D::FreeGrant,
        ),
        (with(free(), json!({"capabilities": ["a"]})), D::FreeGrant),
        (
            with(
                free(),
                json!({"limits": {"mesh.machines": {"max": 3, "scope": "account"},
            "workspaces": {"max": 2, "scope": "installation"}, "mem.items": {"max": 1000, "scope": "installation"}}}),
            ),
            D::FreeGrant,
        ),
    ];
    for (payload, defect) in cases {
        assert_eq!(
            refusal(&signer, &payload),
            Refusal::Payload(defect),
            "{payload}"
        );
    }
}

#[test]
fn capability_bounds_count_scalars_not_bytes() {
    let signer = LocalSigner::new();
    signer
        .check(
            &with(paid(), json!({"capabilities": ["é".repeat(128)]})),
            None,
            NOW,
        )
        .unwrap();
}

#[test]
fn context_binds_audience_policy_and_session() {
    let signer = LocalSigner::new();
    let cases = [
        (
            with(
                paid(),
                json!({"aud": {"project": "demo-other", "env": "test"}}),
            ),
            Refusal::Audience,
        ),
        (
            with(
                paid(),
                json!({"aud": {"project": "demo-local", "env": "live"}}),
            ),
            Refusal::Audience,
        ),
        (
            with(
                paid(),
                json!({"policy_version": 3,
                "limits": {
                    "mesh.machines": {"max": null, "scope": "account"},
                    "workspaces": {"max": null, "scope": "installation"},
                    "mem.items": {"max": null, "scope": "installation"},
                    "teams": {"max": null, "scope": "installation"},
                    "agent.tabs": {"max": null, "scope": "installation"}
                }}),
            ),
            Refusal::PolicyVersion,
        ),
        (
            with(paid(), json!({"policy_epoch": "local-epoch-0"})),
            Refusal::PolicyEpoch,
        ),
        (
            with(
                paid(),
                json!({"enforcement": {"enabled": true, "rollback": false}}),
            ),
            Refusal::Enforcement,
        ),
        (
            with(paid(), json!({"sub": "someone-else"})),
            Refusal::Subject,
        ),
        (
            with(paid(), json!({"inst": "other-installation"})),
            Refusal::Installation,
        ),
    ];
    for (payload, expected) in cases {
        let rejected = signer.check(&payload, None, NOW).unwrap_err();
        assert_eq!(rejected.refusal, expected, "{payload}");
        assert_eq!(rejected.facets.context_matches, Facet::Failed);
    }
    let grant = signer.sign(&paid());
    let invalid = Binding {
        uid: "local-user",
        installation_id: "local/installation",
    };
    let rejected = verify(&grant, &signer.policy(), &invalid, None, NOW).unwrap_err();
    assert_eq!(rejected.refusal, Refusal::Installation);
}

#[test]
fn revision_ordering() {
    let signer = LocalSigner::new();
    let cached = signer.accept(&paid(), NOW);
    let older = signer
        .check(&with(paid(), json!({"revision": 9})), Some(&cached), NOW)
        .unwrap_err();
    assert_eq!(older.refusal, Refusal::Revision);
    let conflict = signer.check(
        &with(
            paid(),
            json!({"perks": {"sponsor": true, "priority_support": true}}),
        ),
        Some(&cached),
        NOW,
    );
    assert_eq!(conflict.unwrap_err().refusal, Refusal::RevisionConflict);
    let newer = signer
        .check(&with(free(), json!({"revision": 11})), Some(&cached), NOW)
        .unwrap();
    assert_eq!(
        newer.facets.revision,
        Some(crate::verify::RevisionOrder::Higher)
    );
    let other_install = with(paid(), json!({"inst": "other-installation", "revision": 1}));
    let unrelated = crate::verify::VerifiedGrant {
        payload: serde_json::from_value(other_install).unwrap(),
        bytes: Vec::new(),
        key_id: String::new(),
    };
    let first = signer.check(&paid(), Some(&unrelated), NOW).unwrap();
    assert_eq!(
        first.facets.revision,
        Some(crate::verify::RevisionOrder::First)
    );
}
