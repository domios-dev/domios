use codeterm_service_core::startup::{Boundary, StartupStep};
use std::cell::RefCell;

#[test]
fn startup_no_global_stall_step_records_both_boundaries() {
    let boundaries = RefCell::new(Vec::new());
    {
        let _step = StartupStep::with_sink("opaque-step", |boundary, _, _| {
            boundaries.borrow_mut().push(boundary);
        });
    }

    assert_eq!(
        boundaries.into_inner(),
        vec![Boundary::Started, Boundary::Completed]
    );
}
