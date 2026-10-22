use super::*;

#[test]
fn close_releases_frontend_then_activation_recreates_it() {
    let mut frontend = Frontend::default();
    assert_eq!(frontend.activate(), Activation::Create);
    frontend.created(true);
    assert!(frontend.close());
    assert!(!frontend.destroyed());
    assert_eq!(frontend, Frontend::Absent);
    assert_eq!(frontend.activate(), Activation::Create);
    frontend.created(true);
    assert_eq!(frontend.activate(), Activation::Raise);
}

#[test]
fn concurrent_activation_has_one_creator_and_coalesces_during_close() {
    let mut frontend = Frontend::default();
    assert_eq!(frontend.activate(), Activation::Create);
    assert_eq!(frontend.activate(), Activation::Wait);
    frontend.created(true);
    assert!(frontend.close());
    assert!(!frontend.close());
    for _ in 0..3 {
        assert_eq!(frontend.activate(), Activation::Wait);
    }
    assert!(frontend.destroyed());
    assert_eq!(frontend.activate(), Activation::Create);
    assert_eq!(frontend.activate(), Activation::Wait);
}

#[test]
fn creation_failure_can_be_retried_by_an_activation_event() {
    let mut frontend = Frontend::default();
    assert_eq!(frontend.activate(), Activation::Create);
    frontend.created(false);
    assert_eq!(frontend.activate(), Activation::Create);
}

#[test]
fn explicit_shutdown_never_reopens_or_retains_daemon() {
    let mut frontend = Frontend::ShuttingDown;
    assert_eq!(frontend.activate(), Activation::Wait);
    assert!(!frontend.close());
    assert!(!frontend.destroyed());
    assert_eq!(frontend, Frontend::ShuttingDown);
    assert!(retain_daemon(None, false));
    assert!(!retain_daemon(None, true));
    assert!(!retain_daemon(Some(0), false));
    assert!(!retain_daemon(Some(i32::MAX), false));
}

#[test]
fn development_activation_arbitrates_only_under_a_separate_app_identity() {
    assert!(!single_instance_enabled(true, "com.domios.app"));
    assert!(single_instance_enabled(true, "com.domios.dev"));
    assert!(single_instance_enabled(true, "com.domios.dev.harbor"));
    assert!(single_instance_enabled(false, "com.domios.app"));
}
