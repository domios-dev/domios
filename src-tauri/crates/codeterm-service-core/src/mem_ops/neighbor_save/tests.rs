use super::*;

fn authority() -> NeighborAuthority {
    NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    }
}

fn body() -> NeighborSaveBody {
    serde_json::from_value(
        serde_json::json!({"kind":"note","area":"project","title":"opaque","body":"opaque body"}),
    )
    .unwrap()
}

#[test]
fn validator_requires_neighbor_authority_and_exact_live_area() {
    let permission = super::matrix::grant();
    assert_eq!(
        validate(None, &permission, body()),
        Err(Refusal::WriteNotAllowed)
    );
    let mut request = body();
    request.area = "private".into();
    assert_eq!(
        validate(Some(&authority()), &permission, request),
        Err(Refusal::AreaNotGranted)
    );
}

#[test]
fn validator_forces_global_unpinned_and_clamps_importance() {
    for (importance, expected) in [(Some(-20.0), 1.0), (Some(20.0), 5.0), (None, 3.0)] {
        let mut request = body();
        request.importance = importance;
        let item = validate(Some(&authority()), &super::matrix::grant(), request).unwrap();
        assert_eq!(item.scope, MemScope::Global);
        assert!(!item.pinned);
        assert_eq!(item.importance, expected);
    }
}

#[test]
fn byte_caps_allow_exact_boundaries_and_refuse_one_more_utf8_byte() {
    let mut request = body();
    request.title = "é".repeat(100);
    request.body = "é".repeat(8192);
    assert!(validate(Some(&authority()), &super::matrix::grant(), request.clone()).is_ok());
    request.body.push('x');
    assert_eq!(
        validate(Some(&authority()), &super::matrix::grant(), request),
        Err(Refusal::TooLarge)
    );
    let mut request = body();
    let used = request.title.len() + request.body.len() + request.area.len() + request.kind.len();
    request.summary = Some("x".repeat(32752 - used));
    assert!(validate(Some(&authority()), &super::matrix::grant(), request.clone()).is_ok());
    request.tags.push("x".into());
    assert_eq!(
        validate(Some(&authority()), &super::matrix::grant(), request),
        Err(Refusal::TooLarge)
    );
}
