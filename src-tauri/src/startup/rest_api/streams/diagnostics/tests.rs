use super::*;

#[test]
fn counts_share_peer_address_and_release_empty_entries() {
    let mut counts = BTreeMap::new();
    assert_eq!(change_count(&mut counts, "127.0.0.1", true), 1);
    assert_eq!(change_count(&mut counts, "127.0.0.1", true), 2);
    assert_eq!(change_count(&mut counts, "::1", true), 1);
    assert_eq!(change_count(&mut counts, "127.0.0.1", false), 1);
    assert_eq!(change_count(&mut counts, "127.0.0.1", false), 0);
    assert!(!counts.contains_key("127.0.0.1"));
    assert_eq!(counts.get("::1"), Some(&1));
}

#[test]
fn dropped_unpolled_body_releases_stream_count() {
    let owner = RequestStreams::default();
    let guard = owner.open(RequestContext {
        path: "/sse".into(),
        peer: "peer".into(),
        client: "page".into(),
    });
    let stream = track(futures_util::stream::pending::<()>(), guard);
    assert_eq!(owner.counts.lock().get("peer"), Some(&1));
    drop(stream);
    assert!(owner.counts.lock().is_empty());
}
