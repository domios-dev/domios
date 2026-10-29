use super::*;

fn context() -> SurfaceInvocationContext {
    SurfaceInvocationContext {
        plugin: "fixture".into(),
        registration: "registration".into(),
        surface: "studio".into(),
        instance: "instance".into(),
        tab: "surface-tab".into(),
        owner: "owner".into(),
        actor: "agent".into(),
        workspace: "workspace".into(),
        team: None,
    }
}

#[test]
fn surface_job_origin_is_exact_to_handler_phases_and_restores_nested_or_unwound_roots() {
    assert!(issue(context()).is_err());
    let _root = RootScope::enter("fixture");
    let ticket = issue(context()).unwrap();
    assert!(current("fixture").is_none());
    assert!(begin(&ticket));
    assert_eq!(current("fixture"), Some(context()));
    assert!(current("foreign").is_none());
    let nested = issue(SurfaceInvocationContext {
        actor: "other".into(),
        ..context()
    })
    .unwrap();
    assert!(begin(&nested));
    assert_eq!(current("fixture").unwrap().actor, "other");
    assert!(end(&nested, false));
    assert_eq!(current("fixture"), Some(context()));
    assert!(end(&ticket, true));
    assert!(current("fixture").is_none());
    assert!(begin(&ticket));
    let failed = std::panic::catch_unwind(|| {
        let _nested = RootScope::enter("fixture");
        assert!(current("fixture").is_none());
        assert!(!begin(&ticket));
        panic!("opaque native unwind");
    });
    assert!(failed.is_err());
    assert_eq!(current("fixture"), Some(context()));
    assert!(end(&ticket, false));
    assert!(current("fixture").is_none());
    assert!(!begin(&ticket));
}

#[test]
fn await_take_requires_exact_native_root_job_and_once_only_resume_or_discard() {
    let _root = RootScope::enter("fixture");
    let root = root_identity();
    assert!(take_mode("key", "job", &root).is_empty());
    {
        let _resume = ResumeScope::enter("key", "job", false);
        assert!(take_mode("foreign", "job", &root).is_empty());
        assert!(take_mode("key", "other-job", &root).is_empty());
        assert!(take_mode("key", "job", "foreign-root").is_empty());
        assert_eq!(take_mode("key", "job", &root), "resume");
        assert!(take_mode("key", "job", &root).is_empty());
    }
    {
        let _discard = ResumeScope::enter("key", "job", true);
        assert_eq!(take_mode("key", "job", &root), "discard");
    }
    let _other = RootScope::enter("fixture");
    let _resume = ResumeScope::enter("key", "job", false);
    assert!(take_mode("key", "job", &root).is_empty());
}
