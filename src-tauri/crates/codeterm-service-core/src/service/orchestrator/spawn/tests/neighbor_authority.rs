use super::*;
use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};

#[test]
fn ordinary_spawn_stamps_neighbor_authority_before_launch_and_later_launches_cannot_clear_it() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let authority = NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    };
    let mut params = SpawnAgentParams {
        neighbor_authority: Some(authority.clone()),
        ..Default::default()
    };
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    initialize_ordinary_pty_agent_tab(tab, &params);
    assert_eq!(tab.neighbor_authority.as_ref(), Some(&authority));
    params.neighbor_authority = None;
    initialize_ordinary_pty_agent_tab(tab, &params);
    assert_eq!(tab.neighbor_authority.as_ref(), Some(&authority));
    params.neighbor_authority = Some(NeighborAuthority {
        uid: "foreign".into(),
        ..authority.clone()
    });
    initialize_ordinary_pty_agent_tab(tab, &params);
    assert_eq!(tab.neighbor_authority.as_ref(), Some(&authority));
}

#[test]
fn external_spawn_parameters_cannot_supply_or_clear_neighbor_authority() {
    let params: SpawnAgentParams = serde_json::from_value(serde_json::json!({"neighbor_authority":{"uid":"foreign","friendship_id":"b".repeat(64),"root_tab_id":null}})).unwrap();
    assert!(params.neighbor_authority.is_none());
}

#[test]
fn host_owned_authority_survives_agent_spawn_parameter_projection() {
    let authority = NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    };
    let params = SpawnAgentParams {
        agent: Some("fixture".into()),
        neighbor_authority: Some(authority.clone()),
        ..Default::default()
    };
    let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(params);
    assert_eq!(
        spec.to_agent_params().unwrap().neighbor_authority.as_ref(),
        Some(&authority)
    );
}
