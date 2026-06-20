#[path = "../../testbench/rust/state_doctor.rs"]
mod state_doctor;

#[test]
#[ignore = "state-dump quality gate is run explicitly by testbench/state-doctor.sh"]
fn state_dump_quality_gate() {
    let root = state_doctor::state_dumps_root();
    assert!(
        root.exists(),
        "state dump library missing at {}",
        root.display()
    );

    assert!(state_doctor::run_state_doctor(&root));
}
