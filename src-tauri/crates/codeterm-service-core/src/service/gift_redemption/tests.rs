use super::*;

#[test]
fn gift_receipts_require_all_five_fields_and_exact_duration_expiry() {
    for duration in ["1m", "3m", "1y", "3y", "lifetime"] {
        let end = if duration == "lifetime" {
            Value::Null
        } else {
            json!(1_900_000_000)
        };
        let valid = json!({"schema_version":1,"status":"redeemed","plan":"pro","duration":duration,"expires_at":end});
        assert!(receipt(valid.clone()).is_ok());
        for field in ["schema_version", "status", "plan", "duration", "expires_at"] {
            let mut missing = valid.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(receipt(missing).is_err(), "missing {field}");
        }
        let mut inverted = valid;
        inverted["expires_at"] = if duration == "lifetime" {
            json!(1_900_000_000)
        } else {
            Value::Null
        };
        assert!(receipt(inverted).is_err());
    }
}

#[test]
fn gift_receipts_refuse_unknown_authority_and_never_echo_response_material() {
    let valid = json!({"schema_version":1,"status":"redeemed","plan":"sponsor","duration":"1y","expires_at":1_900_000_000});
    for (field, value) in [
        ("plan", json!("admin")),
        ("duration", json!("12m")),
        ("schema_version", json!(2)),
        ("status", json!("pending")),
        ("expires_at", json!(-1)),
        ("unexpected", json!("opaque-secret")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert_eq!(receipt(invalid).unwrap_err(), "invalid_response");
    }
    assert_eq!(
        refusal(&json!({"error":{"details":{"reason":"opaque-secret"}}})),
        "unavailable"
    );
    for reason in ["invalid_key", "already_lifetime", "rate_limited"] {
        assert_eq!(
            refusal(&json!({"error":{"details":{"reason":reason}}})),
            reason
        );
    }
}

#[test]
fn gift_request_accepts_only_key_and_expected_account_without_grant_fields() {
    let valid = json!({"key":format!("g1_{}", "a".repeat(43)),"expected_uid":"fixture-user"});
    assert!(serde_json::from_value::<GiftRedemption>(valid.clone())
        .unwrap()
        .valid_key());
    for field in ["plan", "duration", "uid", "grant", "operation"] {
        let mut injected = valid.clone();
        injected[field] = json!("injected");
        assert!(serde_json::from_value::<GiftRedemption>(injected).is_err());
    }
    for key in ["", "g1_short", &format!("g1_{}!", "a".repeat(42))] {
        assert!(!serde_json::from_value::<GiftRedemption>(
            json!({"key":key,"expected_uid":"fixture-user"})
        )
        .unwrap()
        .valid_key());
    }
}

#[test]
fn gift_results_require_the_original_account_and_login_generation() {
    let session = |uid: &str, login_generation| codeterm_account::Session {
        login_generation,
        identity: codeterm_account::Identity {
            uid: uid.into(),
            provider: "firebase".into(),
            email: None,
            display_name: None,
            photo_url: None,
        },
        id_token: codeterm_account::Secret::new("fixture-token"),
        expires_at: None,
    };
    let expected = session("own", 1);
    assert!(same_session(Some(&session("own", 1)), &expected));
    assert!(!same_session(Some(&session("foreign", 1)), &expected));
    assert!(!same_session(Some(&session("own", 2)), &expected));
    assert!(!same_session(None, &expected));
}
