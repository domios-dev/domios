use serde::Serialize;

use super::{BootStage, TimelineState};

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum PaneStatus {
    Working,
    Waiting,
    Idle,
    Dead,
}

pub fn reconcile_status(
    raw: PaneStatus,
    boot_stage: Option<BootStage>,
    agent_state: Option<TimelineState>,
    deterministic_agent_state: bool,
    has_pending_prompt: bool,
) -> PaneStatus {
    if matches!(raw, PaneStatus::Dead | PaneStatus::Waiting) {
        return raw;
    }
    if has_pending_prompt {
        return PaneStatus::Waiting;
    }
    if raw == PaneStatus::Working
        && deterministic_agent_state
        && matches!(
            boot_stage,
            Some(BootStage::AgentReady | BootStage::PtyReattached)
        )
        && agent_state == Some(TimelineState::Idle)
    {
        return PaneStatus::Idle;
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::{BootStage, TimelineState};

    #[test]
    fn focus_repaint_at_ready_prompt_stays_idle() {
        assert_eq!(
            reconcile_status(
                PaneStatus::Working,
                Some(BootStage::AgentReady),
                Some(TimelineState::Idle),
                true,
                false,
            ),
            PaneStatus::Idle
        );
    }

    #[test]
    fn real_long_turn_stays_working() {
        assert_eq!(
            reconcile_status(
                PaneStatus::Working,
                Some(BootStage::AgentReady),
                Some(TimelineState::Working),
                true,
                false,
            ),
            PaneStatus::Working
        );
    }

    #[test]
    fn structural_prompt_return_changes_live_projection_without_raw_transition() {
        let previous = reconcile_status(
            PaneStatus::Working,
            Some(BootStage::AgentReady),
            Some(TimelineState::Working),
            true,
            false,
        );
        let current = reconcile_status(
            PaneStatus::Working,
            Some(BootStage::AgentReady),
            Some(TimelineState::Idle),
            true,
            false,
        );
        assert_eq!((previous, current), (PaneStatus::Working, PaneStatus::Idle));
    }

    #[test]
    fn raw_status_dominates_without_proven_agent_coverage() {
        for stage in [
            None,
            Some(BootStage::AgentLaunching),
            Some(BootStage::SpawnFailed),
        ] {
            assert_eq!(
                reconcile_status(
                    PaneStatus::Working,
                    stage,
                    Some(TimelineState::Idle),
                    true,
                    false,
                ),
                PaneStatus::Working
            );
        }
        assert_eq!(
            reconcile_status(
                PaneStatus::Working,
                Some(BootStage::AgentReady),
                Some(TimelineState::Idle),
                false,
                false,
            ),
            PaneStatus::Working
        );
    }

    #[test]
    fn dead_waiting_and_pending_prompt_keep_raw_semantics() {
        for raw in [PaneStatus::Dead, PaneStatus::Waiting] {
            assert_eq!(
                reconcile_status(
                    raw,
                    Some(BootStage::AgentReady),
                    Some(TimelineState::Idle),
                    true,
                    true,
                ),
                raw
            );
        }
        assert_eq!(
            reconcile_status(
                PaneStatus::Working,
                Some(BootStage::AgentReady),
                Some(TimelineState::Idle),
                true,
                true,
            ),
            PaneStatus::Waiting
        );
    }

    #[test]
    fn restored_ready_prompt_can_veto_repaint_working() {
        assert_eq!(
            reconcile_status(
                PaneStatus::Working,
                Some(BootStage::PtyReattached),
                Some(TimelineState::Idle),
                true,
                false,
            ),
            PaneStatus::Idle
        );
    }
}
