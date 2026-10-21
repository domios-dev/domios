use super::*;

#[test]
fn report_store_cannot_expose_mutable_lower_store() {
    trait AmbiguousIfDerefMut<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfDerefMut<()> for T {}
    impl<T: ?Sized + std::ops::DerefMut> AmbiguousIfDerefMut<u8> for T {}

    let _ = <ReportStore as AmbiguousIfDerefMut<_>>::check;
}

#[test]
fn report_delivery_traces_only_the_first_written_transition() {
    let mut store = ReportStore::new(4);
    let report = store.add("target", "sender", "sender-title", "opaque-body");
    let events = crate::flow_trace::test_capture::capture(|| {
        store.mark_delivered(&report.id);
        store.mark_delivered(&report.id);
        store.mark_delivered("unknown");
    });
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage, "report.written");
    assert_eq!(events[0].tab, "target");
    let lower: &ModelReportStore = &store;
    assert!(lower.pending_for("target").is_empty());
}
