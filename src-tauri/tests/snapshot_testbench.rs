use std::collections::BTreeMap;

#[path = "../../testbench/rust/harness.rs"]
mod harness;

#[test]
fn snapshot_testbench_library_is_green() {
    let root = harness::snapshots_root();
    if !root.exists() {
        panic!("snapshot library missing at {}", root.display());
    }

    let mut providers = BTreeMap::new();
    let cases = harness::discover_cases(&root);
    let results = cases
        .iter()
        .map(|dir| harness::run_case(&root, dir, &mut providers))
        .collect::<Vec<_>>();

    harness::print_results(&results);
    harness::print_scorecard(&results);

    assert!(
        !results.is_empty(),
        "no snapshot testbench cases discovered under {}",
        root.display()
    );
    harness::assert_gating_green(&results);
}
