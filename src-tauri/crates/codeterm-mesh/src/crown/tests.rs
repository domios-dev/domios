use super::*;

#[test]
fn authenticated_origin_is_signed_and_required() {
    let (key, public) = generate_peer_keypair();
    let known = std::collections::HashMap::from([("peer".into(), public)]);
    for origin in [
        AuthenticatedOrigin::Owner,
        AuthenticatedOrigin::Agent,
        AuthenticatedOrigin::Mesh,
        AuthenticatedOrigin::Service,
    ] {
        let token = sign_request_with_origin(&key, "peer", origin).unwrap();
        let claims = verify_request_claims(&token, &known).unwrap();
        assert_eq!(claims.origin, origin);
        let (payload, signature) = token.split_once('.').unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&B64.decode(payload).unwrap()).unwrap();
        value["origin"] = serde_json::json!("Owner");
        if origin != AuthenticatedOrigin::Owner {
            let spoofed = format!(
                "{}.{}",
                B64.encode(serde_json::to_vec(&value).unwrap()),
                signature
            );
            assert!(verify_request_claims(&spoofed, &known).is_err());
        }
        value.as_object_mut().unwrap().remove("origin");
        let stripped = B64.encode(serde_json::to_vec(&value).unwrap());
        let signing = SigningKey::from_bytes(&B64.decode(&key).unwrap().try_into().unwrap());
        let signature = B64.encode(signing.sign(stripped.as_bytes()).to_bytes());
        assert!(verify_request_claims(&format!("{stripped}.{signature}"), &known).is_err());
    }
}

#[test]
fn relay_agent_identity_and_delegation_are_signed() {
    let (key, public) = generate_peer_keypair();
    let known = std::collections::HashMap::from([("forwarder".into(), public)]);
    let agent = AuthenticatedAgent {
        instance_id: "origin-machine".into(),
        tab_id: "agent-tab".into(),
        can_spawn: false,
    };
    let token =
        sign_request_with_agent(&key, "forwarder", AuthenticatedOrigin::Agent, Some(&agent))
            .unwrap();
    assert_eq!(
        verify_request_claims(&token, &known).unwrap().agent,
        Some(agent)
    );
    let (payload, signature) = token.split_once('.').unwrap();
    for field in ["tab_id", "can_spawn"] {
        let mut value: serde_json::Value =
            serde_json::from_slice(&B64.decode(payload).unwrap()).unwrap();
        value["agent"][field] = if field == "can_spawn" {
            serde_json::json!(true)
        } else {
            serde_json::json!("owner-tab")
        };
        let forged = format!(
            "{}.{}",
            B64.encode(serde_json::to_vec(&value).unwrap()),
            signature
        );
        assert!(verify_request_claims(&forged, &known).is_err());
    }
}

#[test]
fn test_peer_sign_verify_roundtrip() {
    let (priv_key, pub_key) = generate_peer_keypair();
    let token = sign_request(&priv_key, "peer-abc").unwrap();
    assert!(!token.is_empty());
    assert!(token.contains('.'));

    let mut known = std::collections::HashMap::new();
    known.insert("peer-abc".to_string(), pub_key);
    let sender = verify_signed_request(&token, &known).unwrap();
    assert_eq!(sender, "peer-abc");
}

#[test]
fn test_peer_sign_wrong_key_rejected() {
    let (priv_key_a, _pub_key_a) = generate_peer_keypair();
    let (_priv_key_b, pub_key_b) = generate_peer_keypair();
    let token = sign_request(&priv_key_a, "peer-a").unwrap();

    let mut known = std::collections::HashMap::new();
    known.insert("peer-a".to_string(), pub_key_b);
    assert!(verify_signed_request(&token, &known).is_err());
}

#[test]
fn test_peer_sign_unknown_peer_rejected() {
    let (priv_key, _pub_key) = generate_peer_keypair();
    let token = sign_request(&priv_key, "peer-unknown").unwrap();

    let known = std::collections::HashMap::new();
    assert!(verify_signed_request(&token, &known).is_err());
}

#[test]
fn public_key_from_private_matches_keypair_pub() {
    let (priv_key, pub_key) = generate_peer_keypair();
    let derived = public_key_from_private(&priv_key).unwrap();
    assert_eq!(derived, pub_key);
}

#[test]
fn public_key_from_private_rejects_garbage() {
    assert!(public_key_from_private("not-base64!").is_err());
    assert!(public_key_from_private(&B64.encode([0u8; 16])).is_err());
}

#[test]
fn nonce_from_token_roundtrip() {
    let (priv_key, _) = generate_peer_keypair();
    let token = sign_request(&priv_key, "peer-abc").unwrap();
    let nonce = nonce_from_token(&token).unwrap();
    assert!(!nonce.is_empty(), "nonce should be a non-empty UUID");
    let token2 = sign_request(&priv_key, "peer-abc").unwrap();
    let nonce2 = nonce_from_token(&token2).unwrap();
    assert_ne!(nonce, nonce2, "each sign produces a unique nonce");
}
