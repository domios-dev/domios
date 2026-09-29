use crate::agent::provider::{ComposerCursorPosition, ProviderReadinessStrategy};

#[test]
fn structural_visible_anywhere_launch_keeps_provider_frame_wait() {
    let structural = ProviderReadinessStrategy::StructuralComposer {
        timeout: std::time::Duration::from_secs(30),
    };

    assert!(
        super::super::owner_prompt_wait::should_wait_for_provider_frame(
            &structural,
            ComposerCursorPosition::VisibleAnywhere,
            false,
        )
    );
}

#[test]
fn structural_bottom_region_launch_uses_launch_gate_without_provider_frame_wait() {
    let structural = ProviderReadinessStrategy::StructuralComposer {
        timeout: std::time::Duration::from_secs(30),
    };

    assert!(
        !super::super::owner_prompt_wait::should_wait_for_provider_frame(
            &structural,
            ComposerCursorPosition::BottomRegion,
            false,
        )
    );
}
