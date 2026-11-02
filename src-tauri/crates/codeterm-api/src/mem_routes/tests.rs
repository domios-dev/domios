use super::*;
use axum::{extract::State, http::HeaderValue, Json};
use codeterm_service_core::{
    multiplexer::{state::MultiplexerState, types::NeighborAuthority},
    service::test_fixture::ServiceFixture,
};

#[tokio::test]
async fn actor_and_selected_tab_headers_cannot_forge_save_author() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let friend = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&friend).unwrap().neighbor_authority = Some(NeighborAuthority {
        uid: "friend".into(),
        friendship_id: "friendship".into(),
        root_tab_id: None,
    });
    let fixture = ServiceFixture::new(mux);
    fixture.seed_neighbor_author("friend", "friendship", "remote-friend");
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().memory.auto_extract = false;
    let owner_token = state
        .svc
        .frontend_pty_env(Some(&owner))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let friend_token = state
        .svc
        .frontend_pty_env(Some(&friend))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {owner_token}")).unwrap(),
    );
    headers.insert(
        "x-codeterm-actor",
        HeaderValue::from_static("neighbor:forged"),
    );
    headers.insert("x-codeterm-tab-id", HeaderValue::from_str(&friend).unwrap());
    let body = serde_json::from_value(serde_json::json!({"kind":"fact","title":"Authored fact","body":"a complete body","skip_extract":true})).unwrap();
    let Json(saved) = items::mem_save(State(state.clone()), headers.clone(), Json(body))
        .await
        .unwrap();
    let stored = state
        .svc
        .mem
        .get(saved["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.author.kind,
        codeterm_mem::store::MemAuthorKind::Owner
    );
    assert_eq!(
        serde_json::to_value(&stored).unwrap()["author"]["kind"],
        "owner"
    );
    let history = state.svc.mem.get_item_history(&stored.id).unwrap();
    assert_eq!(history[0].author, stored.author);
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {friend_token}")).unwrap(),
    );
    headers.insert("x-codeterm-actor", HeaderValue::from_static("owner"));
    assert_eq!(
        memory_author(&state, &headers).await.unwrap(),
        codeterm_mem::store::MemAuthor::neighbor("remote-friend".into(), "friendship".into())
    );
}
