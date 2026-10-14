use super::*;

async fn decode_text(headers: &HeaderMap, body: &Bytes) -> Result<Value, WebhookRejection> {
    decode(headers, body, None)
        .await
        .map(|decoded| decoded.fields)
}

fn form(fields: &[(&str, &str)]) -> (HeaderMap, Bytes) {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "multipart/form-data; boundary=coreapp-fixture"
            .parse()
            .unwrap(),
    );
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!(
            "--coreapp-fixture\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str("--coreapp-fixture--\r\n");
    (headers, Bytes::from(body))
}

fn file_part(name: &str, filename: Option<&str>, media: &str, data: &[u8]) -> Vec<u8> {
    let filename = filename
        .map(|filename| format!("; filename=\"{filename}\""))
        .unwrap_or_default();
    let mut part = format!(
        "--coreapp-fixture\r\nContent-Disposition: form-data; name=\"{name}\"{filename}\r\nContent-Type: {media}\r\n\r\n"
    )
    .into_bytes();
    part.extend_from_slice(data);
    part.extend_from_slice(b"\r\n");
    part
}

fn with_parts(parts: &[Vec<u8>], fields: &[(&str, &str)]) -> (HeaderMap, Bytes) {
    let (headers, body) = form(fields);
    let mut combined = parts.concat();
    combined.extend_from_slice(&body);
    (headers, Bytes::from(combined))
}

#[tokio::test]
async fn coreapp_multipart_preserves_transcription_and_metadata() {
    let fields = [
        ("transcription", "Hello, ring!\nSecond line \u{1f680}"),
        ("recordedAt", "1789412400000"),
        ("client", "ring"),
        ("test", "true"),
    ];
    let (headers, body) = form(&fields);
    let payload = decode_text(&headers, &body).await.unwrap();
    for (name, value) in fields {
        assert_eq!(payload[name], value);
    }
}

#[tokio::test]
async fn binary_audio_is_not_exposed_to_the_plugin() {
    let audio = file_part("audio", Some("recording.m4a"), "audio/mp4", b"\xff\x00\xfe");
    let (headers, body) = with_parts(&[audio], &[("transcription", "text"), ("client", "ring")]);
    let decoded = decode(&headers, &body, None).await.unwrap();
    assert_eq!(decoded.fields["transcription"], "text");
    assert!(decoded.fields.get("audio").is_none());
    assert!(decoded.audio.is_none());
}

#[tokio::test]
async fn missing_or_incorrect_boundary_is_an_observable_error() {
    let (mut headers, body) = form(&[("client", "ring")]);
    for value in ["multipart/form-data", "multipart/form-data; boundary=wrong"] {
        headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
        let error = decode_text(&headers, &body).await.unwrap_err();
        assert_eq!(error.code, "invalid_multipart");
    }
}

#[tokio::test]
async fn duplicate_fields_are_rejected() {
    let (headers, body) = form(&[("client", "ring"), ("client", "other")]);
    assert_eq!(
        decode_text(&headers, &body).await.unwrap_err().code,
        "invalid_form_fields"
    );
}

#[tokio::test]
async fn invalid_utf8_text_is_rejected_without_lossy_replacement() {
    let (headers, body) = form(&[("transcription", "@")]);
    let mut bytes = body.to_vec();
    let offset = bytes.iter().position(|byte| *byte == b'@').unwrap();
    bytes[offset] = 0xff;
    assert_eq!(
        decode_text(&headers, &Bytes::from(bytes))
            .await
            .unwrap_err()
            .code,
        "invalid_form_fields"
    );
}

#[tokio::test]
async fn form_count_field_and_total_size_are_bounded() {
    let large = "x".repeat(MAX_TEXT_FIELD_BYTES + 1);
    let (headers, body) = form(&[("transcription", &large)]);
    assert_eq!(
        decode_text(&headers, &body).await.unwrap_err().status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let names: Vec<_> = (0..=MAX_FIELDS).map(|i| format!("field-{i}")).collect();
    let fields: Vec<_> = names.iter().map(|name| (name.as_str(), "value")).collect();
    let (headers, body) = form(&fields);
    assert_eq!(
        decode_text(&headers, &body).await.unwrap_err().status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let oversized = Bytes::from(vec![0; MAX_WEBHOOK_BODY_BYTES + 1]);
    assert_eq!(
        decode_text(&headers, &oversized).await.unwrap_err().status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn json_media_type_is_case_insensitive_and_unknown_types_stay_rejected() {
    let mut headers = HeaderMap::new();
    let body = Bytes::from_static(br#"{"client":"ring"}"#);
    for value in ["Application/JSON; charset=utf-8", "application/vendor+json"] {
        headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
        assert_eq!(
            decode_text(&headers, &body).await.unwrap()["client"],
            "ring"
        );
    }
    headers.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
    assert_eq!(
        decode_text(&headers, &body).await.unwrap_err().status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    headers.remove(header::CONTENT_TYPE);
    assert_eq!(
        decode_text(&headers, &body).await.unwrap_err().status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

const CAP: Option<usize> = Some(8 * 1024 * 1024);

#[tokio::test]
async fn pebble_audio_receiver_decode_extracts_one_audio_part_beside_text_fields() {
    let audio = file_part("audio", Some("ring note.m4a"), "Audio/MP4", b"\xff\x00\xfe");
    let (headers, body) = with_parts(&[audio], &[("transcription", "text"), ("client", "ring")]);
    let decoded = decode(&headers, &body, CAP).await.unwrap();
    assert_eq!(decoded.fields["transcription"], "text");
    assert!(decoded.fields.get("audio").is_none());
    let audio = decoded.audio.expect("audio part");
    assert_eq!(audio.media_type, "audio/mp4");
    assert_eq!(audio.file_name, "ring_note.m4a");
    assert_eq!(&audio.bytes[..], b"\xff\x00\xfe");
}

#[tokio::test]
async fn pebble_audio_receiver_decode_names_an_unnamed_part_from_its_media_type() {
    let audio = file_part("audio", None, "audio/mpeg", b"id3");
    let (headers, body) = with_parts(&[audio], &[("client", "ring")]);
    let audio = decode(&headers, &body, CAP).await.unwrap().audio.unwrap();
    assert_eq!(audio.file_name, "audio.mp3");
    let hostile = file_part("audio", Some("../../etc/passwd"), "audio/wav", b"riff");
    let (headers, body) = with_parts(&[hostile], &[("client", "ring")]);
    let audio = decode(&headers, &body, CAP).await.unwrap().audio.unwrap();
    assert!(!audio.file_name.contains('/') && !audio.file_name.starts_with('.'));
}

#[tokio::test]
async fn pebble_audio_receiver_decode_skips_non_audio_files_and_empty_audio() {
    let image = file_part("photo", Some("pic.png"), "image/png", b"png");
    let empty = file_part("audio", Some("none.m4a"), "audio/mp4", b"");
    let (headers, body) = with_parts(&[image, empty], &[("client", "ring")]);
    let decoded = decode(&headers, &body, CAP).await.unwrap();
    assert!(decoded.audio.is_none());
    assert!(decoded.fields.get("photo").is_none());
}

#[tokio::test]
async fn pebble_audio_receiver_decode_rejects_a_second_audio_part() {
    let first = file_part("audio", Some("a.m4a"), "audio/mp4", b"a");
    let second = file_part("audio2", Some("b.m4a"), "audio/mp4", b"b");
    let (headers, body) = with_parts(&[first, second], &[("client", "ring")]);
    let error = decode(&headers, &body, CAP).await.err().expect("rejected");
    assert_eq!(error.code, "invalid_form_fields");
}

#[tokio::test]
async fn pebble_audio_receiver_decode_enforces_the_cap_and_the_text_limit_beside_it() {
    let over = file_part("audio", Some("a.m4a"), "audio/mp4", &[7; 1024]);
    let (headers, body) = with_parts(&[over], &[("client", "ring")]);
    let error = decode(&headers, &body, Some(1023))
        .await
        .err()
        .expect("over cap");
    assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);

    let audio = file_part("audio", Some("a.m4a"), "audio/mp4", b"a");
    let filler = file_part(
        "attachment",
        Some("big.bin"),
        "application/octet-stream",
        &vec![0; MAX_WEBHOOK_BODY_BYTES],
    );
    let (headers, body) = with_parts(&[audio, filler], &[("client", "ring")]);
    let error = decode(&headers, &body, CAP)
        .await
        .err()
        .expect("text limit");
    assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn pebble_audio_receiver_decode_accepts_audio_above_the_extractor_default_limit() {
    let three_mib = vec![1_u8; 3 * 1024 * 1024];
    let audio = file_part("audio", Some("long.m4a"), "audio/mp4", &three_mib);
    let (headers, body) = with_parts(&[audio], &[("client", "ring")]);
    let audio = decode(&headers, &body, CAP).await.unwrap().audio.unwrap();
    assert_eq!(audio.bytes.len(), three_mib.len());
}

#[tokio::test]
async fn pebble_audio_receiver_json_bodies_never_gain_the_audio_allowance() {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    let mut json = br#"{"pad":""#.to_vec();
    json.extend_from_slice(&vec![b'x'; MAX_WEBHOOK_BODY_BYTES]);
    json.extend_from_slice(br#""}"#);
    let error = decode(&headers, &Bytes::from(json), CAP)
        .await
        .err()
        .expect("json over the text limit");
    assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
}
