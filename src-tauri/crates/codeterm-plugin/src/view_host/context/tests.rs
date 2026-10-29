use super::*;
#[test]
fn invocation_scope_nests_and_restores_without_an_echoed_context_setter() {
    assert!(current_view_call_context("outer").is_none());
    let outer = ViewCallContext::panel("outer");
    {
        let _outer = ViewInvocationScope::enter(outer.clone());
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
