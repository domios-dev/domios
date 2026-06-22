#[path = "../../testbench/rust/state_calibration.rs"]
mod state_calibration;

#[test]
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
