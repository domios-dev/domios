use super::*;
use crate::multiplexer::state::MultiplexerState;

#[test]
fn neighbor_authority_survives_saved_session_round_trip_without_a_live_agent() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let authority = NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    };
    mux.get_tab_mut(&tab_id).unwrap().neighbor_authority = Some(authority.clone());
    let serialized = serde_json::to_vec(&mux.to_session_snapshot()).unwrap();
    let saved = serde_json::from_slice(&serialized).unwrap();
    let restored = MultiplexerState::from_session_snapshot(&saved);
    assert_eq!(
        restored
            .get_tab(&tab_id)
            .unwrap()
            .neighbor_authority
            .as_ref(),
        Some(&authority)
    );
}
