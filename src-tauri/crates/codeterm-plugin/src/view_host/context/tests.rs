use super::*;
#[test]
fn invocation_scope_nests_and_restores_without_an_echoed_context_setter() {
    assert!(current_view_call_context("outer").is_none());
    assert!(!has_view_call_context());
    let outer = ViewCallContext::panel("outer");
    {
        let _outer = ViewInvocationScope::enter(outer.clone());
        assert!(has_view_call_context());
        assert!(current_view_call_context("foreign").is_none());
        assert_eq!(current_view_call_context("outer"), Some(outer.clone()));
        {
            let _inner = ViewInvocationScope::enter(ViewCallContext::panel("inner"));
            assert!(current_view_call_context("outer").is_none());
            assert_eq!(current_view_call_context("inner").unwrap().plugin, "inner");
        }
        assert_eq!(current_view_call_context("outer"), Some(outer));
    }
    assert!(current_view_call_context("outer").is_none());
}

#[test]
fn invocation_wrapper_rejects_foreign_plugin_and_restores_on_unwind() {
    let ran = std::cell::Cell::new(false);
    assert!(
        with_view_call_context("other", &ViewCallContext::panel("outer"), || {
            ran.set(true);
            Ok(())
        })
        .is_err()
    );
    assert!(!ran.get());
    let result = std::panic::catch_unwind(|| {
        let _: Result<(), super::super::ViewError> =
            with_view_call_context("outer", &ViewCallContext::panel("outer"), || {
                panic!("fixture unwind")
            });
    });
    assert!(result.is_err());
    assert!(current_view_call_context("outer").is_none());
}

#[test]
fn native_ambient_scope_probe_restores_after_nested_unwind() {
    assert!(!has_view_call_context());
    let _outer = ViewInvocationScope::enter(ViewCallContext::panel("outer"));
    let result = std::panic::catch_unwind(|| {
        let _inner = ViewInvocationScope::enter(ViewCallContext::panel("inner"));
        assert!(has_view_call_context());
        panic!("opaque unwind");
    });
    assert!(result.is_err());
    assert!(has_view_call_context());
    assert!(current_view_call_context("outer").is_some());
}
