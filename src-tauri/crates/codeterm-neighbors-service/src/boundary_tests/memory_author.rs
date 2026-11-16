use crate::prelude::*;
use crate::{
    mem::store::MemAuthor,
    multiplexer::{state::MultiplexerState, types::NeighborAuthority},
};
use codeterm_service_core::mem_ops::author::*;

#[tokio::test]
async fn authenticated_tab_authority_precedes_worker_and_owner_identity() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let friend = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&friend).unwrap().neighbor_authority = Some(NeighborAuthority {
        uid: "owner-uid".into(),
        friendship_id: "friendship".into(),
        root_tab_id: None,
    });
    let fixture = crate::test_support::fixture(mux);
    let svc = &fixture.service;
    fixture.seed_neighbor_author("owner-uid", "friendship", "friend-uid");
    *svc.ext::<OwnerMemoryAuthor>().0.write() = Some("owner-uid".into());
    assert_eq!(
        mem_author_for_caller(svc, Some(&owner)).await.unwrap(),
        MemAuthor::owner(Some("owner-uid".into()))
    );
    assert_eq!(
        mem_author_for_caller(svc, Some(&friend)).await.unwrap(),
        MemAuthor::neighbor("friend-uid".into(), "friendship".into())
    );
    assert_eq!(
        mem_author_for_caller(svc, None).await.unwrap(),
        MemAuthor::owner(Some("owner-uid".into()))
    );
}

#[tokio::test]
async fn other_neighbor_rpc_writes_retain_unresolved_neighbor_authorship_without_lookup() {
    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().neighbor_authority = Some(NeighborAuthority {
        uid: "local-owner".into(),
        friendship_id: "friendship".into(),
        root_tab_id: None,
    });
    let fixture = crate::test_support::fixture(mux);
    fixture.seed_neighbor_author("local-owner", "friendship", "");
    let author = mem_author_for_rpc(&fixture.service, Some(&tab), "mem.save")
        .await
        .unwrap();
    assert!(author.is_neighbor());
    assert_eq!(author.account_uid, None);
    assert_eq!(author.friendship_id.as_deref(), Some("friendship"));
    let item = crate::mem::store::NewMemItem {
        kind: crate::mem::store::MemKind::Fact,
        scope: crate::mem::store::MemScope::Global,
        title: "opaque unresolved author".into(),
        body: "opaque body".into(),
        summary: None,
        area: Some("project".into()),
        tags: vec![],
        source: None,
        source_path: None,
        importance: 3.0,
        pinned: false,
    };
    let before = fixture.service.mem.stats().unwrap().total_items;
    let (scope, result) = crate::mem::store::with_author(author.clone(), || {
        (
            crate::mem::store::current_author(),
            fixture.service.mem.save(item),
        )
    });
    assert_eq!(scope, author);
    assert!(result
        .unwrap_err()
        .contains("neighbor author requires account and friendship"));
    assert_eq!(fixture.service.mem.stats().unwrap().total_items, before);
    assert!(
        mem_author_for_rpc(&fixture.service, Some(&tab), "mem.neighbor_save")
            .await
            .unwrap_err()
            .starts_with("neighbor_memory_author_unavailable:")
    );
}
