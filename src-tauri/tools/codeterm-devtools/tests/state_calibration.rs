#[path = "common/test_support.rs"]
mod test_support;

#[path = "../../../../testbench/rust/state_calibration.rs"]
mod state_calibration;

#[test]
#[ignore = "Full state-corpus calibration benchmark; run deliberately with --test state_calibration -- --ignored"]
fn state_calibration_bench() {
    let root = state_calibration::state_dumps_root();
    assert!(
        root.exists(),
        "state dump library missing at {}",
        root.display()
    );

    let summary = state_calibration::run_state_calibration(&root);
    assert!(summary.dumps > 0, "no state dumps discovered");
}
