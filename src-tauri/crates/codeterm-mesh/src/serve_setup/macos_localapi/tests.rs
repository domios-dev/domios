use super::*;

#[test]
fn probe_capture_preserves_utf8_split_across_http_chunks() {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nA\r\n{\"text\":\"\xc3\r\n3\r\n\xa9\"}\r\n0\r\n\r\n";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
    assert_eq!(
        decode_probe_output(&encoded),
        Some(serde_json::json!({"text": "\u{e9}"}))
    );
    assert!(decode_probe_output("not base64!").is_none());
}

#[test]
fn content_length_response_decodes_before_json() {
    let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}";
    let body = decode_response(raw).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"ok": true})
    );
}

#[test]
fn multi_chunk_response_decodes_json_across_chunk_boundaries() {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4;fixture=yes\r\n{\"ok\r\n7\r\n\":true}\r\n0\r\nX-Fixture: done\r\n\r\n";
    let body = decode_response(raw).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"ok": true})
    );
}

#[test]
fn response_decoder_refuses_truncated_ambiguous_and_failed_responses() {
    for raw in [
        &b"HTTP/1.1 403 Forbidden\r\nContent-Length: 2\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n{}"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}xx0\r\n\r\n"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\nextra"[..],
        &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffffffffffff\r\n"[..],
    ] {
        assert!(decode_response(raw).is_none());
    }
}

#[test]
fn response_decoder_bounds_headers_bodies_and_chunk_count() {
    let mut headers = b"HTTP/1.1 200 OK\r\nX-Large: ".to_vec();
    headers.extend(vec![b'x'; MAX_HEADERS]);
    headers.extend_from_slice(b"\r\n\r\n{}");
    assert!(decode_response(&headers).is_none());
    let mut body = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
    body.extend(vec![b' '; MAX_BODY + 1]);
    assert!(decode_response(&body).is_none());
    assert!(decode_response(&vec![b' '; MAX_RESPONSE + 1]).is_none());
    let mut chunks = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for _ in 0..MAX_CHUNKS {
        chunks.extend_from_slice(b"1\r\nx\r\n");
    }
    chunks.extend_from_slice(b"0\r\n\r\n");
    assert!(decode_response(&chunks).is_none());
}
