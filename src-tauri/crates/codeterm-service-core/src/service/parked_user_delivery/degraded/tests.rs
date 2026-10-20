use super::*;

#[test]
fn only_never_written_pre_ready_input_settles_on_degraded_launch_evidence() {
    for stage in [
        BootStage::LaunchDeadlineElapsed,
        BootStage::LaunchBindingUnconfirmed,
        BootStage::SpawnFailed,
    ] {
        assert!(can_settle_degraded(
            UserSendState::Queued,
            true,
            false,
            false,
            false,
            Some(stage),
        ));
        for state in [
            UserSendState::Claimed,
            UserSendState::Uncertain,
            UserSendState::BodyLease,
        ] {
            assert!(!can_settle_degraded(
                state,
                true,
                false,
                false,
                false,
                Some(stage)
            ));
        }
        for (held, handoff, ready, written) in [
            (false, false, false, false),
            (true, true, false, false),
            (true, false, true, false),
            (true, false, false, true),
        ] {
            assert!(!can_settle_degraded(
                UserSendState::Queued,
                held,
                handoff,
                ready,
                written,
                Some(stage),
            ));
        }
    }
}

#[test]
fn normal_boot_and_owner_prompt_keep_originals_held() {
    for stage in BootStage::HAPPY_PATH.into_iter().map(Some).chain([
        None,
        Some(BootStage::AwaitingOwnerPrompt),
        Some(BootStage::PreparingRoleInstructions),
        Some(BootStage::DeliveringInstructions),
        Some(BootStage::PtyReattached),
    ]) {
        assert!(!can_settle_degraded(
            UserSendState::Queued,
            true,
            false,
            false,
            false,
            stage,
        ));
    }
}
