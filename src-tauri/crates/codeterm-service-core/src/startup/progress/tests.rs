use super::{Boundary, StartupStep};
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

#[test]
fn startup_phase_preserves_identity_and_records_duration_on_unwind() {
    let boundaries = RefCell::new(Vec::new());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _step = StartupStep::with_sink("opaque-phase", |boundary, name, elapsed| {
            boundaries.borrow_mut().push((boundary, name, elapsed));
        });
        panic!("fixture failure");
    }));
    assert!(result.is_err());
    let boundaries = boundaries.into_inner();
    assert_eq!(boundaries.len(), 2);
    assert_eq!(boundaries[0], (Boundary::Started, "opaque-phase", None));
    assert_eq!(boundaries[1].0, Boundary::Completed);
    assert_eq!(boundaries[1].1, "opaque-phase");
    assert!(boundaries[1].2.is_some());
}
