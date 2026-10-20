use super::*;

#[test]
fn chunk_decode_moves_the_original_allocation_into_json() {
    let mut bytes = Vec::with_capacity(1024);
    bytes.extend_from_slice("opaque λ\n".as_bytes());
    let pointer = bytes.as_ptr();
    let capacity = bytes.capacity();
    let serde_json::Value::String(chunk) = chunk_value(bytes) else {
        panic!("expected string argument");
    };
    assert_eq!(chunk.as_ptr(), pointer);
    assert_eq!(chunk.capacity(), capacity);
    assert_eq!(chunk, "opaque λ\n");
}

#[test]
fn lossy_chunk_decode_preserves_original_record_positions() {
    let bytes = b"\xff\nnext\n".to_vec();
    let starts = crate::jsonl_window::record_starts(&bytes);
    assert_eq!(
        chunk_value(bytes),
        serde_json::Value::String("�\nnext\n".into())
    );
    let mut rows = vec![Default::default(), Default::default()];
    crate::jsonl_window::anchor_rows_from_starts(&mut rows, &[Some(0), Some(1)], &starts, 100);
    assert_eq!(rows[0].record_start, Some(100));
    assert_eq!(rows[1].record_start, Some(102));
}
