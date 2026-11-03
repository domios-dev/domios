use crate::plan::{Task, TaskStatus};
use crate::service_plan::{review_gate, PlanError};

pub(in crate::service_plan) fn validate(
    task: Option<&Task>,
    current: &TaskStatus,
    next: &TaskStatus,
    incoming_label: Option<Option<&str>>,
    has_tab: bool,
    dispatch: impl FnOnce(&Task) -> Result<(), PlanError>,
    terminal: impl FnOnce() -> Result<(), PlanError>,
) -> Result<(), PlanError> {
    let invariants_on =
        std::env::var("CODETERM_STATE_INVARIANTS").map_or(true, |value| value != "off");
    if invariants_on {
        if matches!(next, TaskStatus::InProgress) {
            if !has_tab {
                return Err(PlanError::InProgressRequiresTab);
            }
            if matches!(current, TaskStatus::Pending) {
                if let Some(task) = task {
                    dispatch(task)?;
                }
            }
        }
        if matches!(next, TaskStatus::InReview) {
            if let Some(task) = task {
                review_gate::validate_enter_review(task)?;
            }
        }
        if matches!(next, TaskStatus::Blocked)
            && incoming_label
                .flatten()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .is_none()
        {
            return Err(PlanError::BlockedRequiresLabel);
        }
    }
    if current == next {
        return Ok(());
    }
    if matches!(next, TaskStatus::Done | TaskStatus::Cancelled) {
        terminal()?;
    }
    if matches!(current, TaskStatus::InReview) && matches!(next, TaskStatus::Paused) {
        return Err(PlanError::ReviewHoldRequired);
    }
    if matches!(current, TaskStatus::InReview) && !matches!(next, TaskStatus::Cancelled) {
        return Err(PlanError::TransitionDenied {
            from: current.clone(),
            to: next.clone(),
        });
    }
    if matches!(next, TaskStatus::Paused) {
        return Err(PlanError::TaskPausedSystemOnly);
    }
    Ok(())
}
