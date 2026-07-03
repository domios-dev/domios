use super::*;

#[test]
fn parse_synthetic_cast_and_inject_offsets() {
    let path = repo_root().join("testbench/replays/synthetic/repaint_proof_v1/raw.cast");
    let cast = parse_cast_file(&path).expect("parse synthetic cast");
    assert!(cast.width > 0);
    assert!(!cast.events.is_empty());

    let mut ob = OutputBuffer::new(1 << 20);
    let start = ob.total_written("pty-test");
    let (nbytes, end) = inject_output_events(&mut ob, "pty-test", &cast.events);
    assert_eq!(nbytes, output_byte_count(&cast.events));
    assert_eq!(end, start + nbytes as u64);

    let (chunk, total) = ob.read_since("pty-test", start);
    assert_eq!(total, end);
    assert_eq!(chunk.len(), nbytes);
}

#[test]
fn inject_preserves_resume_offset_semantics() {
    let mut ob = OutputBuffer::new(1 << 20);
    let events = vec![
        CastEvent::Output {
            at_ms: 0,
            bytes: b"hello ".to_vec(),
        },
        CastEvent::Output {
            at_ms: 50,
            bytes: b"world".to_vec(),
        },
    ];
    inject_output_events(&mut ob, "p", &events);
    let (tail, total) = ob.read_since("p", 6);
    assert_eq!(tail, b"world");
    assert_eq!(total, 11);
}

#[test]
fn resolve_replay_path_rejects_escape() {
    let root = repo_root();
    let ok = resolve_replay_path(&root, "synthetic/repaint_proof_v1/raw.cast");
    assert!(ok.is_ok(), "{ok:?}");
    let bad = resolve_replay_path(&root, "../../../etc/passwd");
    assert!(bad.is_err());
}

#[test]
fn parse_cast_decodes_o_b64_rows() {
    let text = r#"{"version": 2, "width": 80, "height": 24}
[0.0, "o+b64", "SGVsbG8="]
"#;
    let cast = parse_cast_text(text).unwrap();
    assert_eq!(cast.events.len(), 1);
    match &cast.events[0] {
        CastEvent::Output { bytes, .. } => assert_eq!(bytes, b"Hello"),
        _ => panic!("expected output"),
    }
}

#[test]
fn parse_cast_ignores_input_rows_with_object_payloads() {
    let text = r#"{"version": 2, "width": 80, "height": 24}
[0.0, "i", {"source":"pty.write","text":"ignored"}]
[0.1, "o", "visible"]
"#;
    let cast = parse_cast_text(text).unwrap();
    assert_eq!(cast.events.len(), 1);
    match &cast.events[0] {
        CastEvent::Output { bytes, .. } => assert_eq!(bytes, b"visible"),
        _ => panic!("expected output"),
    }
}
