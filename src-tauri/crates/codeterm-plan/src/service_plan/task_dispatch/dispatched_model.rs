use super::{DispatchedAttemptMetadata, DispatchStartupOwner};
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn begin_task_dispatch_with_policy_inner(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        reserved_attempt_id: Option<&str>,
        authorized_gates: Option<Vec<String>>,
        placement: crate::plan::WorkerPlacement,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
        startup_owner_observation_made: bool,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            reserved_attempt_id,
            authorized_gates,
            placement,
            observed_startup_owner,
            observed_startup_owner_is_live,
            startup_owner_observation_made,
            None,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch_with_reserved_attempt_and_dispatched_model(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        attempt_id: &str,
        model: Option<String>,
        reasoning_effort: Option<String>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            Some(attempt_id),
            None,
            crate::plan::WorkerPlacement::IsolatedLane,
            None,
            false,
            false,
            Some(DispatchedAttemptMetadata {
                model,
                reasoning_effort,
            }),
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch_with_reserved_attempt_and_gates_and_observation_and_dispatched_model(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        attempt_id: &str,
        authorized_gates: Vec<String>,
        placement: crate::plan::WorkerPlacement,
        model: Option<String>,
        reasoning_effort: Option<String>,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            Some(attempt_id),
            Some(authorized_gates),
            placement,
            observed_startup_owner,
            observed_startup_owner_is_live,
            true,
            Some(DispatchedAttemptMetadata {
                model,
                reasoning_effort,
            }),
            now_ms,
        )
    }
}
