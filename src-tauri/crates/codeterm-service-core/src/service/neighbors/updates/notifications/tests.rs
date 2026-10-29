use super::*;
use serde_json::json;

#[test]
fn arrival_and_proven_acceptance_notify_once_across_restore() {
    let list = json!({"incoming":[{"request_id":"r","displayName":"Pat","uid":"peer"}],
        "friends":[{"friendship_id":"f","displayName":"Lin","uid":"friend"}]});
    let accepted = [Acceptance {
        event_id: "event".into(),
        friendship_id: "f".into(),
    }];
    let mut seen = Seen::default();
    let notices = changes(&mut seen, &list, &accepted).unwrap();
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.title.as_str())
            .collect::<Vec<_>>(),
        ["Pat wants to be your Neighbor", "Lin accepted your request"]
    );
    let mut restored = serde_json::from_value(serde_json::to_value(seen).unwrap()).unwrap();
    assert!(changes(&mut restored, &list, &accepted).unwrap().is_empty());
    assert!(changes(
        &mut Seen::default(),
        &json!({"incoming":[],"friends":list["friends"]}),
        &[]
    )
    .unwrap()
    .is_empty());
    assert!(changes(
        &mut Seen::default(),
        &list,
        &[Acceptance {
            event_id: "foreign-event".into(),
            friendship_id: "foreign".into(),
        }]
    )
    .unwrap()
    .iter()
    .all(|notice| notice.event_id.starts_with("request:")));
}

#[test]
fn local_request_identity_uses_email_and_never_exposes_uid_when_profile_exists() {
    assert_eq!(
        name(&json!({"uid":"opaque","displayName":"Anya","email":"anya@example.test"})).unwrap(),
        "Anya (anya@example.test)"
    );
    assert_eq!(
        name(&json!({"uid":"opaque","displayName":null,"email":"anya@example.test"})).unwrap(),
        "anya@example.test"
    );
    assert_eq!(
        name(&json!({"uid":"opaque","displayName":"Anya","email":null})).unwrap(),
        "Anya"
    );
    assert_eq!(
        name(&json!({"uid":"opaque","displayName":null,"email":null})).unwrap(),
        "opaque"
    );
}
