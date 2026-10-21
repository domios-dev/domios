//! Team lifecycle, orchestrator refresh and batch, orchestration pairs and lane cleanup, above the core.

pub mod lane_clean;
pub mod orchestration_pair;
mod orchestration_port;
pub mod orchestrator;
mod orchestrator_onboard;
pub mod team;

#[cfg(test)]
mod core_hub_tests;
#[cfg(test)]
mod test_fixture;

pub use orchestration_pair::{
    OrchestrationPairExt, OrchestrationPairParams, OrchestrationPairResult,
};
pub use orchestration_port::register_orchestration;
pub use orchestrator::{
    batch, maybe_refresh_after_reflection, orchestrator_refresh, RefreshOutcome, RefreshRequest,
    RefreshTrigger, RefreshTuning,
};
#[cfg(test)]
pub(crate) use orchestrator_onboard::deliver_orchestrator_onboarding;
pub use orchestrator_onboard::{
    make_orchestrator, maybe_deliver_orchestrator_promotion_onboarding,
};
pub use team::{
    add_team_member, adopt_agent_for_team, adopt_plan_for_team, resume_team_plan,
    team_member_brief, transfer_team_plan, unarchive_team_plan, validate_active_team_plan_bindings,
    validate_plan_team_binding, AddTeamMemberParams, AddTeamMemberResult, CartographerAttachLocks,
    OrchestratorAttachLocks, TeamCloseResult, TeamExt,
};
#[cfg(any(test, feature = "test-support"))]
pub use team::{close_team_with_state_change, rename_team_core, rename_team_with_state_change};
