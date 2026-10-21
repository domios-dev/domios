use super::*;
#[test]
fn lane_checkpoint_ref_component_rejects_ref_path_injection() {
    assert!(!safe_ref_component("../foreign"));
    assert!(safe_ref_component("attempt-12_ab"));
    assert_eq!(
        checkpoint_turn(
            "refs/domios/checkpoints/attempt",
            "refs/domios/checkpoints/attempt/4"
        ),
        Some(4)
    );
    assert_eq!(
        checkpoint_turn(
            "refs/domios/checkpoints/attempt",
            "refs/domios/checkpoints/attempt/metadata"
        ),
        None
    );
    assert_eq!(
        checkpoint_turn(
            "refs/domios/checkpoints/attempt",
            "refs/domios/checkpoints/attempt/1/nested"
        ),
        None
    );
}
