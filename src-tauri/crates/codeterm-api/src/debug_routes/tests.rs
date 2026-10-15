use super::event_stats_snapshot;
use crate::stream_census::{StreamCensus, StreamKind, StreamScope};
use codeterm_service_core::service::MuxEmitStats;
use std::sync::atomic::Ordering;

#[test]
fn event_stats_adds_streams_and_preserves_mux_emit_observations() {
    let stats = MuxEmitStats::default();
    stats.requests.store(9, Ordering::Relaxed);
    stats.fires.store(7, Ordering::Relaxed);
    stats.suppressed.store(2, Ordering::Relaxed);
    stats.pending_since_ms.store(100, Ordering::Relaxed);
    stats.frozen.store(true, Ordering::Relaxed);
    let census = StreamCensus::default();
    let guard = census.open(StreamKind::Events, StreamScope::Local);
    let value = event_stats_snapshot(&stats, true, 140, &census);
    assert_eq!(value["emit_requests"], 9);
    assert_eq!(value["emit_fires"], 7);
    assert_eq!(value["emit_suppressed"], 2);
    assert_eq!(value["pending"], true);
    assert_eq!(value["frozen"], true);
    assert_eq!(value["pending_since_ms"], 40);
    assert_eq!(value["streams"]["live"], 1);
    assert_eq!(value["streams"]["opened"], 1);
    assert_eq!(value["streams"]["closed"], 0);
    drop(guard);
    let value = event_stats_snapshot(&stats, false, 140, &census);
    assert_eq!(value["pending_since_ms"], 0);
    assert_eq!(value["streams"]["live"], 0);
    assert_eq!(value["streams"]["closed"], 1);
}
