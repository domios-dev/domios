use super::*;
#[test]
fn blueprint_v2_call_context_retains_exact_parent_identity() {
    let mut context = CallContext::default();
    let pin = CallPin {
        id: "mem:callee".into(),
        hash: "a".repeat(64),
    };
    context
        .enter("call-node", &pin, JournalMarker::Step(4))
        .unwrap();
    let parent = context.parent().unwrap();
    assert_eq!(parent.marker, JournalMarker::Step(4));
    assert_eq!(parent.frame.id, "mem:callee");
    assert_eq!(parent.frame.hash, "a".repeat(64));
    assert_eq!(
        context.enter("recursive", &pin, JournalMarker::Step(5)),
        Err(JournalReason::ContractViolation)
    );
    context.exit();
    assert!(context.parent().is_none());
}
