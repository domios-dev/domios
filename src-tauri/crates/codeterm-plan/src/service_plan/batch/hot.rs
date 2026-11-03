use super::{BatchOp, BatchRequest};

pub(super) fn supports(request: &BatchRequest) -> bool {
    request.ops.iter().all(|op| {
        matches!(
            op,
            BatchOp::SetTaskBody { .. }
                | BatchOp::SetTaskSections { .. }
                | BatchOp::SetTaskStatus { .. }
                | BatchOp::AddMilestone { .. }
        )
    })
}
