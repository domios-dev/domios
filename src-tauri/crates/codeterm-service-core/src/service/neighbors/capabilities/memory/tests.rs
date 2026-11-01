use super::*;

#[test]
fn memory_areas_cannot_include_foreign_or_unassigned_items_implicitly() {
    let areas = vec!["project".into()];
    assert!(area_allowed(&areas, Some("project")));
    assert!(!area_allowed(&areas, Some("private")));
    assert!(!area_allowed(&areas, None));
    assert!(!area_allowed(&[], Some("project")));
    assert!(area_allowed(&["(none)".into()], None));
}

#[tokio::test]
async fn saved_grants_validate_write_areas_even_without_a_read_area_filter() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let permission = crate::mem_ops::neighbor_save::matrix::grant();
    let authority = crate::multiplexer::types::NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    };
    let body = serde_json::from_value(
        serde_json::json!({"kind":"fact","area":"project","title":"opaque","body":"opaque body"}),
    )
    .unwrap();
    let item =
        crate::mem_ops::neighbor_save::validate(Some(&authority), &permission, body).unwrap();
    fixture.service.mem.insert_raw(&item).unwrap();
    assert!(fixture
        .service
        .validate_neighbor_memory_areas(&permission)
        .await
        .is_ok());
    let mut unknown = permission;
    unknown.memory_write_areas = vec!["unknown".into()];
    assert_eq!(
        fixture
            .service
            .validate_neighbor_memory_areas(&unknown)
            .await,
        Err(codeterm_neighbors::Error::MemoryAreaUnknown)
    );
}
