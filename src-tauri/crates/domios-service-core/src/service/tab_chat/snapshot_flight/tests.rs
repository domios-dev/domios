use super::*;

fn key(tab: &str) -> SnapshotKey {
    SnapshotKey {
        tab_id: tab.into(),
        provider_id: "supplied-provider".into(),
        session_id: "supplied-session".into(),
        binding_nonce: Some("supplied-nonce".into()),
        incarnation: Some("supplied-incarnation".into()),
        path: "supplied.jsonl".into(),
    }
}

#[test]
fn concurrent_same_source_callers_share_one_parse_and_result_allocation() {
    let flights = SnapshotFlights::default();
    let (first, leader) = flights.acquire(key("tab"));
    let (second, duplicate_leader) = flights.acquire(key("tab"));
    assert!(leader);
    assert!(!duplicate_leader);
    assert!(Arc::ptr_eq(&first, &second));
    let result = Producer {
        flight: first.clone(),
        completed: false,
    }
    .finish(SessionSnapshot {
        total: 42,
        ..Default::default()
    });
    let joined = second.wait().unwrap();
    assert!(Arc::ptr_eq(&result, &joined));
    assert_eq!(joined.total, 42);
    drop(first);
    drop(second);
    assert!(
        flights.acquire(key("tab")).1,
        "completed results are not retained as a history cache"
    );
}

#[test]
fn other_tabs_and_changed_authority_never_wait_for_an_active_parse() {
    let flights = SnapshotFlights::default();
    let (_blocked, _) = flights.acquire(key("one"));
    assert_eq!(
        flights
            .run(key("two"), || SessionSnapshot {
                total: 7,
                ..Default::default()
            })
            .unwrap()
            .total,
        7
    );
    for changed in [
        SnapshotKey {
            binding_nonce: Some("new-nonce".into()),
            ..key("one")
        },
        SnapshotKey {
            incarnation: Some("new-file".into()),
            ..key("one")
        },
    ] {
        assert!(flights.acquire(changed).1);
    }
}

#[test]
fn an_aborted_producer_releases_joined_callers_with_an_error() {
    let flights = SnapshotFlights::default();
    let (first, _) = flights.acquire(key("one"));
    let (joined, _) = flights.acquire(key("one"));
    drop(Producer {
        flight: first,
        completed: false,
    });
    assert!(joined.wait().is_err());
}
