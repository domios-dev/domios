use super::*;

fn document(version: u64) -> Value {
    json!({"schema_version":1,"version":version,"updated_at":100,"access":"paid","can_invite":true,
        "friends":[],"incoming":[],"outgoing":[],"own_keys":[],
        "referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}})
}

#[test]
fn account_generation_and_monotonic_versions_gate_every_projection() {
    let mut store = AccountSnapshot::default();
    store.select("owner", 1);
    assert!(store.apply("owner", 1, Snapshot::parse(document(2)).unwrap()));
    for (uid, generation, version) in [
        ("owner", 1, 2),
        ("owner", 1, 1),
        ("other", 1, 3),
        ("owner", 2, 3),
    ] {
        assert!(!store.apply(uid, generation, Snapshot::parse(document(version)).unwrap()));
    }
    store.select("owner", 1);
    assert_eq!(store.version("owner", 1), Some(2));
    store.select("owner", 2);
    assert!(store.read("owner", 1, "friendsList", "desk", 100).is_none());
    assert!(store.apply("owner", 2, Snapshot::parse(document(1)).unwrap()));
}

#[test]
fn projection_preserves_referral_and_active_keys_and_expires_invites_without_a_cloud_read() {
    let mut value = document(1);
    value["own_keys"] = json!([{"verifier_key_id":"vk_active","enc_key_id":null,"installation_id":null,"machine_label":null,"created_at":1}]);
    value["outgoing"] = json!([{"id":"a".repeat(64),"typed_email":"peer@example.test","created_at":1,"state":"pending"}]);
    value["referral_trial"] = json!({"state":"granted","trial_end":1000,"inviter_uid":"inviter","friend_request_id":"b".repeat(64)});
    let snapshot = Snapshot::parse(value).unwrap();
    assert_eq!(
        snapshot.list(30 * 86400)["outgoing"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(snapshot.list(30 * 86400 + 1)["outgoing"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(snapshot.list(100)["referral_trial"]["state"], "granted");
    assert_eq!(
        snapshot.read("friendsOwnKeys", "desk", 100).unwrap()[0]["verifier_key_id"],
        "vk_active"
    );
    assert!(snapshot.list(100).get("own_keys").is_none());
}

#[test]
fn malformed_projection_bounds_and_referral_status_are_refused() {
    for (field, value) in [
        ("schema_version", json!(2)),
        ("version", json!(-1)),
        ("version", json!(MAX_SAFE_INTEGER + 1)),
        ("friends", json!({})),
        ("can_invite", json!("true")),
        ("own_keys", json!([{"verifier_key_id":"bad/key"}])),
    ] {
        let mut invalid = document(1);
        invalid[field] = value;
        assert!(Snapshot::parse(invalid).is_err(), "{field}");
    }
    let mut invalid = document(1);
    invalid["referral_trial"]["state"] = json!("fabricated");
    assert!(Snapshot::parse(invalid).is_err());
}

#[test]
fn non_receiver_one_hour_idle_and_focus_reads_make_zero_backend_calls() {
    let mut store = AccountSnapshot::default();
    store.select("owner", 1);
    assert!(store.apply("owner", 1, Snapshot::parse(document(1)).unwrap()));
    let mut backend_calls = 0;
    for now in [0, 360, 720, 1080, 1440, 1800, 2160, 2520, 2880, 3240, 3600] {
        for route in ["friendsList", "friendsOwnKeys", "friendsReceiverStatus"] {
            if store.read("owner", 1, route, "non-receiver", now).is_none() {
                backend_calls += 1;
            }
        }
    }
    assert_eq!(backend_calls, 0);
    assert!(store
        .read("owner", 1, "friendsReceiverStatus", "non-receiver", 3600)
        .unwrap()["receivers"]
        .as_array()
        .unwrap()
        .is_empty());
}
