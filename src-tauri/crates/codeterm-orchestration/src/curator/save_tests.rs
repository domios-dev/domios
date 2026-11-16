use super::*;

#[test]
fn curator_save_candidates_discard_deleted_blueprints() {
    let mut state = codeterm_types::plan::ControlLoop::default();
    for id in ["blueprint_live", "blueprint_deleted", "plan-skill:kept"] {
        state
            .save_candidates
            .insert(id.into(), ["episode-a".into(), "episode-b".into()].into());
        state
            .save_candidate_shapes
            .insert(id.into(), "shape".into());
    }
    retain_live_blueprints(&mut state, &["blueprint_live".into()].into());
    assert!(!state.save_candidates.contains_key("blueprint_deleted"));
    assert!(!state
        .save_candidate_shapes
        .contains_key("blueprint_deleted"));
    assert_eq!(state.save_candidates.len(), 2);
    assert_eq!(state.save_candidate_shapes.len(), 2);
}
