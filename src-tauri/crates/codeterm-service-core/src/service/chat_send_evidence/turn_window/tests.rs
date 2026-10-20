use super::*;

#[test]
fn bounded_append_window_never_reads_stale_empty_or_truncated_anchors() {
    let cap = codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES;
    let from = 48_789_141;
    for total in [from - 1, from, from + cap + 1, from + 48 * 1024 * 1024] {
        assert_eq!(
            bounded_append_window(from, total, |_| panic!("unbounded read")),
            None::<u64>
        );
    }
    assert_eq!(
        bounded_append_window(from, from + cap, |offset| offset),
        Some(from)
    );
    assert_eq!(
        bounded_append_window(from, from + 1, |offset| offset),
        Some(from)
    );
}
