use super::*;
use codeterm_service_core::config::schema::AppearanceLook;

fn proposal(id: &str, sender: Option<&str>, state: AppearanceState) -> AppearanceProposal {
    let look = AppearanceLook::from_config(&AppConfig::default());
    AppearanceProposal {
        id: id.into(),
        rationale: id.into(),
        proposed_by_installation: sender.map(str::to_owned),
        tweak: false,
        look: look.clone(),
        previous: look,
        previous_active: None,
        state,
        applied_at: (state != AppearanceState::Proposed).then_some(1),
    }
}

#[test]
fn remote_replaces_only_its_own_disposable_pending_proposals() {
    use AppearanceState::*;
    let mut config = AppConfig::default();
    let mut active = proposal("active", Some("source"), Applied);
    active.previous_active = Some("linked".into());
    config.appearance_active = Some(active.id.clone());
    config.appearance_proposals = vec![
        proposal("local", None, Proposed),
        proposal("foreign", Some("other"), Proposed),
        proposal("linked", Some("source"), Proposed),
        proposal("old", Some("source"), Proposed),
        proposal("recent", Some("source"), Proposed),
        active,
    ];
    retain(&mut config, proposal("new", Some("source"), Proposed)).unwrap();
    assert_eq!(
        config
            .appearance_proposals
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["local", "foreign", "linked", "recent", "active", "new"],
    );
}

#[test]
fn full_history_refuses_remote_growth_and_leaves_room_for_owner_actions() {
    use AppearanceState::*;
    let mut config = AppConfig::default();
    for id in 0..PROPOSAL_LIMIT - 1 {
        retain(
            &mut config,
            proposal(&format!("owner-{id}"), None, Proposed),
        )
        .unwrap();
    }
    retain(&mut config, proposal("foreign", Some("other"), Proposed)).unwrap();
    let error = retain(&mut config, proposal("new", Some("source"), Proposed)).unwrap_err();
    assert_eq!(error.0, StatusCode::CONFLICT);
    assert_eq!(config.appearance_proposals.len(), PROPOSAL_LIMIT);
    assert_eq!(config.appearance_proposals[0].id, "owner-0");

    retain(&mut config, proposal("local", None, Proposed)).unwrap();
    assert_eq!(config.appearance_proposals.len(), PROPOSAL_LIMIT);
    assert_eq!(config.appearance_proposals[0].id, "owner-0");
    assert!(!config
        .appearance_proposals
        .iter()
        .any(|entry| entry.id == "foreign"));
    assert_eq!(config.appearance_proposals.last().unwrap().id, "local");
}
