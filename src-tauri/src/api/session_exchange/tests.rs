use super::import_target_allowed;

#[test]
fn fixture_import_cannot_reach_production_or_ordinary_dev() {
    assert!(!import_target_allowed(
        false,
        7686,
        "-upgrade-restore-fixture"
    ));
    assert!(!import_target_allowed(
        true,
        7685,
        "-upgrade-restore-fixture"
    ));
    assert!(!import_target_allowed(true, 7686, ""));
    assert!(!import_target_allowed(true, 7686, "-another-team"));
    assert!(import_target_allowed(
        true,
        17686,
        "-upgrade-restore-fixture"
    ));
}
