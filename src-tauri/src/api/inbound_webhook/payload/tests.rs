use super::*;

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

#[tokio::test]
async fn coreapp_multipart_preserves_transcription_and_metadata() {
    let fields = [
        ("transcription", "Hello, ring!\nSecond line \u{1f680}"),
        ("recordedAt", "1789412400000"),
        ("client", "ring"),
        ("test", "true"),
    ];
    let (headers, body) = form(&fields);
    let payload = decode(&headers, &body).await.unwrap();
    for (name, value) in fields {
        assert_eq!(payload[name], value);
    }
}

#[tokio::test]
async fn binary_audio_is_not_exposed_to_the_plugin() {
    let (headers, body) = form(&[("transcription", "text"), ("client", "ring")]);
    let mut combined = b"--coreapp-fixture\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"recording.m4a\"\r\nContent-Type: audio/mp4\r\n\r\n\xff\x00\xfe\r\n".to_vec();
    combined.extend_from_slice(&body);
    let payload = decode(&headers, &Bytes::from(combined)).await.unwrap();
    assert_eq!(payload["transcription"], "text");
    assert!(payload.get("audio").is_none());
}

#[tokio::test]
async fn missing_or_incorrect_boundary_is_an_observable_error() {
    let (mut headers, body) = form(&[("client", "ring")]);
    for value in ["multipart/form-data", "multipart/form-data; boundary=wrong"] {
        headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
        let error = decode(&headers, &body).await.unwrap_err();
        assert_eq!(error.code, "invalid_multipart");
    }
}

#[tokio::test]
async fn duplicate_fields_are_rejected() {
    let (headers, body) = form(&[("client", "ring"), ("client", "other")]);
    assert_eq!(
        decode(&headers, &body).await.unwrap_err().code,
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
        decode(&headers, &Bytes::from(bytes))
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
        decode(&headers, &body).await.unwrap_err().status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let names: Vec<_> = (0..=MAX_FIELDS).map(|i| format!("field-{i}")).collect();
    let fields: Vec<_> = names.iter().map(|name| (name.as_str(), "value")).collect();
    let (headers, body) = form(&fields);
    assert_eq!(
        decode(&headers, &body).await.unwrap_err().status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let oversized = Bytes::from(vec![0; MAX_WEBHOOK_BODY_BYTES + 1]);
    assert_eq!(
        decode(&headers, &oversized).await.unwrap_err().status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn json_media_type_is_case_insensitive_and_unknown_types_stay_rejected() {
    let mut headers = HeaderMap::new();
    let body = Bytes::from_static(br#"{"client":"ring"}"#);
    for value in ["Application/JSON; charset=utf-8", "application/vendor+json"] {
        headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
        assert_eq!(decode(&headers, &body).await.unwrap()["client"], "ring");
    }
    headers.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
    assert_eq!(
        decode(&headers, &body).await.unwrap_err().status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    headers.remove(header::CONTENT_TYPE);
    assert_eq!(
        decode(&headers, &body).await.unwrap_err().status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}
