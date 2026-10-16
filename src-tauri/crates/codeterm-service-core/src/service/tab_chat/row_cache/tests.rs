use super::*;
use crate::agent::chat_types::ChatMsg;
use crate::service::tab_chat::resume_cursor::ResumeCursor;
use std::cell::Cell;

fn epoch() -> ChatEpoch {
    ChatEpoch {
        source_session_id: "own".into(),
        session_binding_nonce: Some("nonce".into()),
    }
}

fn snapshot(content: &str, start: u64, end: u64) -> SessionSnapshot {
    SessionSnapshot {
        messages: vec![ChatMsg {
            id: content.into(),
            content: content.into(),
            ..Default::default()
        }],
        start_offset: start,
        end_offset: end,
        total: end,
        bytes_read: end - start,
        ..Default::default()
    }
}

fn put(cache: &RowCache, tab: &str, content: &str) {
    let cursor = ResumeCursor::Bytes {
        epoch: epoch(),
        incarnation: "file".into(),
        offset: 100,
    }
    .encode()
    .unwrap();
    cache.insert(tab, &epoch(), cursor, &snapshot(content, 20, 100));
}

#[test]
fn unchanged_returns_cached_rows_without_parser_reads() {
    let cache = RowCache::default();
    put(&cache, "tab", "retained");
    let reads = Cell::new(0);
    let entry = cache.get("tab", &epoch(), "file", 100).unwrap();
    let result = entry
        .resume(&epoch(), "file", 100, |_| {
            reads.set(reads.get() + 1);
            None
        })
        .unwrap();
    assert_eq!(result.messages[0].content, "retained");
    assert_eq!(result.start_offset, 20);
    assert_eq!(result.bytes_read, 0);
    assert_eq!(reads.get(), 0);
}

#[test]
fn append_reads_only_the_append_and_keeps_the_cached_window() {
    let cache = RowCache::default();
    put(&cache, "tab", "retained");
    let reads = Cell::new(0);
    let entry = cache.get("tab", &epoch(), "file", 120).unwrap();
    let result = entry
        .resume(&epoch(), "file", 120, |known| {
            reads.set(reads.get() + 1);
            assert_eq!(known.end_offset, 100);
            Some(snapshot("appended", 100, 120))
        })
        .unwrap();
    assert_eq!(
        result
            .messages
            .iter()
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        ["retained", "appended"]
    );
    assert_eq!(
        (result.start_offset, result.end_offset, result.bytes_read),
        (20, 120, 20)
    );
    assert_eq!(reads.get(), 1);
    let cursor = ResumeCursor::Bytes {
        epoch: epoch(),
        incarnation: "file".into(),
        offset: 120,
    }
    .encode()
    .unwrap();
    cache.insert("tab", &epoch(), cursor, &result);
    let next = cache.get("tab", &epoch(), "file", 120).unwrap();
    let next = next
        .resume(&epoch(), "file", 120, |_| panic!("unchanged append reread"))
        .unwrap();
    assert_eq!(next.messages.len(), 2);
    assert_eq!(next.bytes_read, 0);
}

#[test]
fn fifteen_tab_warm_cycles_have_no_parser_reads_and_flat_cache_size() {
    let cache = RowCache::default();
    let content = "x".repeat(8_000);
    for tab in 0..15 {
        put(&cache, &tab.to_string(), &content);
    }
    let warm_bytes = cache.charged_bytes();
    let reads = Cell::new(0);
    for phase in 0..3 {
        let started = std::time::Instant::now();
        for tab in 0..15 {
            let entry = cache.get(&tab.to_string(), &epoch(), "file", 100).unwrap();
            let result = entry
                .resume(&epoch(), "file", 100, |_| {
                    reads.set(reads.get() + 1);
                    None
                })
                .unwrap();
            assert_eq!(result.messages[0].content, content);
        }
        assert_eq!(cache.len(), 15);
        assert_eq!(cache.charged_bytes(), warm_bytes);
        eprintln!("row_cache phase={phase} revisits=15 parser_reads={} charged_bytes={warm_bytes} elapsed_us={}", reads.get(), started.elapsed().as_micros());
    }
    assert_eq!(reads.get(), 0);
    put(&cache, "16", "sixteen");
    put(&cache, "17", "seventeen");
    assert_eq!(cache.len(), 16);
    assert!(cache.get("0", &epoch(), "file", 100).is_none());
}

#[test]
fn authority_identity_truncation_and_large_append_drop_stale_rows() {
    let cache = RowCache::default();
    for (current_epoch, incarnation, total) in [
        (
            ChatEpoch {
                source_session_id: "other".into(),
                ..epoch()
            },
            "file",
            100,
        ),
        (
            ChatEpoch {
                session_binding_nonce: Some("new".into()),
                ..epoch()
            },
            "file",
            100,
        ),
        (epoch(), "replacement", 100),
        (epoch(), "file", 99),
        (
            epoch(),
            "file",
            100 + super::super::session_window::SNAPSHOT_WINDOW_BYTES,
        ),
    ] {
        put(&cache, "tab", "old");
        assert!(cache
            .get("tab", &current_epoch, incarnation, total)
            .is_none());
        assert_eq!(cache.len(), 0);
        put(&cache, "tab", "refilled");
        assert_eq!(
            cache
                .get("tab", &epoch(), "file", 100)
                .unwrap()
                .snapshot
                .messages[0]
                .content,
            "refilled"
        );
    }
}

#[test]
fn entry_and_byte_limits_evict_least_recently_used_and_reject_oversize() {
    let cache = RowCache::with_limits(2, usize::MAX);
    put(&cache, "a", "a");
    put(&cache, "b", "b");
    assert!(cache.get("a", &epoch(), "file", 100).is_some());
    put(&cache, "c", "c");
    assert!(cache.get("b", &epoch(), "file", 100).is_none());
    assert_eq!(cache.len(), 2);
    let charge = cache.charged_bytes() / 2;
    let cache = RowCache::with_limits(16, charge);
    put(&cache, "a", "a");
    put(&cache, "b", "b");
    assert_eq!(cache.len(), 1);
    assert!(cache.charged_bytes() <= charge);
    put(&cache, "b", &"x".repeat(charge * 2));
    assert_eq!(cache.len(), 0);
}

#[test]
fn shutdown_tab_drops_rows_without_requiring_a_tailer() {
    let registry = super::super::registry::ChatRegistry::default();
    put(&registry.row_cache, "tab", "retained");
    registry.shutdown_tab("tab");
    assert_eq!(registry.row_cache.len(), 0);
}

#[test]
fn cached_rows_do_not_replay_runtime_observations() {
    let cache = RowCache::default();
    let mut original = snapshot("retained", 20, 100);
    original.runtime_observations.push(Default::default());
    let cursor = ResumeCursor::Bytes {
        epoch: epoch(),
        incarnation: "file".into(),
        offset: 100,
    }
    .encode()
    .unwrap();
    cache.insert("tab", &epoch(), cursor, &original);
    let entry = cache.get("tab", &epoch(), "file", 100).unwrap();
    let cached = entry
        .resume(&epoch(), "file", 100, |_| panic!("reread"))
        .unwrap();
    assert!(cached.runtime_observations.is_empty());
    assert_eq!(original.runtime_observations.len(), 1);
}

#[test]
fn partial_append_updates_file_length_without_advancing_served_end() {
    let cache = RowCache::default();
    put(&cache, "tab", "retained");
    let entry = cache.get("tab", &epoch(), "file", 110).unwrap();
    let partial = entry
        .resume(&epoch(), "file", 110, |_| {
            Some(SessionSnapshot {
                start_offset: 100,
                end_offset: 100,
                total: 110,
                ..Default::default()
            })
        })
        .unwrap();
    let cursor = ResumeCursor::Bytes {
        epoch: epoch(),
        incarnation: "file".into(),
        offset: 100,
    }
    .encode()
    .unwrap();
    cache.insert("tab", &epoch(), cursor, &partial);
    let entry = cache.get("tab", &epoch(), "file", 110).unwrap();
    assert_eq!(entry.file_length(), 110);
    let unchanged = entry
        .resume(&epoch(), "file", 110, |_| panic!("partial reread"))
        .unwrap();
    assert_eq!(unchanged.end_offset, 100);
    assert_eq!(unchanged.messages[0].content, "retained");
}
