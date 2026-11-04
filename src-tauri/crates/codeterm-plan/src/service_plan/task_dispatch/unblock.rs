use super::super::PlanError;

pub(in crate::service_plan) fn task_startup_status(
    status: &crate::plan::TaskStatus,
) -> Result<crate::plan::TaskStatus, PlanError> {
    match status {
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress => {
            Ok(status.clone())
        }
        crate::plan::TaskStatus::Blocked => Ok(crate::plan::TaskStatus::Pending),
        _ => Err(PlanError::TransitionDenied {
            from: status.clone(),
            to: crate::plan::TaskStatus::Pending,
        }),
    }
}

pub(in crate::service_plan) fn task_respawn_status(
    status: &crate::plan::TaskStatus,
) -> Result<crate::plan::TaskStatus, PlanError> {
    match status {
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress => {
            Ok(status.clone())
        }
        crate::plan::TaskStatus::Blocked | crate::plan::TaskStatus::InReview => {
            Ok(crate::plan::TaskStatus::Pending)
        }
        _ => Err(PlanError::TransitionDenied {
            from: status.clone(),
            to: crate::plan::TaskStatus::Pending,
        }),
    }
}
