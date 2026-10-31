use super::*;

fn assert_pin_wins(profile: &str) {
    let pin = PathBuf::from("private-worker/codeterm");
    for local in [None, Some(PathBuf::from(format!("{profile}/bin/codeterm")))] {
        assert_eq!(
            select_instance_cli_path(Some(pin.clone()), || local),
            Some(pin.clone())
        );
    }
}

fn assert_unpinned_fallback(profile: &str) {
    for local in [None, Some(PathBuf::from(format!("{profile}/bin/codeterm")))] {
        assert_eq!(select_instance_cli_path(None, || local.clone()), local);
    }
}

#[test]
fn debug_pin_wins_over_absent_or_stale_local_cli() {
    assert_pin_wins(".domios-dev");
}

#[test]
fn release_pin_wins_over_absent_or_stale_local_cli() {
    assert_pin_wins(".domios");
}

#[test]
fn debug_without_pin_preserves_local_fallback() {
    assert_unpinned_fallback(".domios-dev");
}

#[test]
fn release_without_pin_preserves_local_fallback() {
    assert_unpinned_fallback(".domios");
}

#[test]
fn lifecycle_pin_does_not_probe_local_installation() {
    let pin = PathBuf::from("private-worker/codeterm");
    assert_eq!(
        select_instance_cli_path(Some(pin.clone()), || panic!("local CLI was probed")),
        Some(pin)
    );
}
