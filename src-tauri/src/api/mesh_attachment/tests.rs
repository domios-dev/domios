use super::*;
use crate::mesh::types::{AccessLevel, Permission, RelayResponse};
use axum::http::Method;
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ct-mesh-attachment-{label}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn payload(kind: AttachmentKind, name: &str, bytes: &[u8]) -> IngestRequest {
    IngestRequest {
        kind,
        name: name.to_string(),
        size: bytes.len() as u64,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

fn ok_response(value: serde_json::Value) -> RelayResponse {
    RelayResponse {
        ok: true,
        status: 200,
        data: Some(value),
        error: None,
    }
}

fn send_body(text: &str) -> serde_json::Value {
    serde_json::json!({ "text": text })
}

fn host_is_file(candidate: &str) -> bool {
    std::path::Path::new(candidate).is_file()
}

#[test]
fn mesh_attachment_transfer_decodes_bounded_payload() {
    let (name, bytes) =
        decode_ingest(&payload(AttachmentKind::File, "photo.png", b"png-bytes")).unwrap();
    assert_eq!(name, "photo.png");
    assert_eq!(bytes, b"png-bytes");
}

#[test]
fn mesh_attachment_transfer_refuses_oversized_payloads() {
    let mut claimed = payload(AttachmentKind::Voice, "note.webm", b"x");
    claimed.size = MESH_ATTACHMENT_MAX_BYTES + 1;
    let refusal = decode_ingest(&claimed).unwrap_err();
    assert_eq!(refusal.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let smuggled = IngestRequest {
        kind: AttachmentKind::File,
        name: "big.bin".into(),
        size: 3,
        data: "A".repeat(ENCODED_MAX_BYTES + 4),
    };
    assert_eq!(
        decode_ingest(&smuggled).unwrap_err().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[test]
fn mesh_attachment_transfer_refuses_malformed_payloads() {
    let not_base64 = IngestRequest {
        kind: AttachmentKind::File,
        name: "a.png".into(),
        size: 4,
        data: "not base64!!".into(),
    };
    let empty = payload(AttachmentKind::File, "a.png", b"");
    let mut mismatched = payload(AttachmentKind::File, "a.png", b"abcd");
    mismatched.size = 9;
    for request in [not_base64, empty, mismatched] {
        let refusal = decode_ingest(&request).unwrap_err();
        assert!(matches!(refusal, IngestRefusal::Malformed(_)), "{refusal:?}");
        assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
    }
}

#[test]
fn mesh_attachment_transfer_strips_unsafe_names() {
    for hostile in ["../../etc/passwd", "..\\..\\evil.png", "/abs/path.png", "C:\\x.png"] {
        let (name, _) = decode_ingest(&payload(AttachmentKind::File, hostile, b"x")).unwrap();
        assert!(!name.contains('/') && !name.contains('\\'), "{name}");
        assert!(!name.starts_with('.'), "{name}");
    }
}

#[tokio::test]
async fn mesh_attachment_transfer_voice_and_image_land_in_target_upload_dir() {
    let origin = scratch("origin");
    let target = scratch("target");
    let voice_source = origin.join("1a2b3c4d-voice-note.webm");
    let image_source = origin.join("5e6f7a8b-pasted image.png");
    std::fs::write(&voice_source, b"opus-frames").unwrap();
    std::fs::write(&image_source, b"png-frames").unwrap();

    let mut stored = Vec::new();
    for (source, kind) in [
        (&voice_source, AttachmentKind::Voice),
        (&image_source, AttachmentKind::File),
    ] {
        let request = build_ingest_request(source, kind).unwrap();
        let wire = serde_json::to_value(&request).unwrap();
        let received: IngestRequest = serde_json::from_value(wire).unwrap();
        assert_eq!(received.kind, kind);
        let (name, bytes) = decode_ingest(&received).unwrap();
        let path = crate::api::web_routes::store_upload(&target, &name, &bytes)
            .await
            .unwrap();
        assert!(path.starts_with(&target), "{}", path.display());
        assert_ne!(&path, source);
        assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(source).unwrap());
        stored.push(path.to_string_lossy().to_string());
    }
    assert_ne!(stored[0], stored[1]);

    let voice_source_str = voice_source.to_string_lossy().to_string();
    let voice = parse_ingest_response(
        &voice_source_str,
        ok_response(serde_json::json!({
            "path": stored[0],
            "kind": "voice",
            "size": 11,
            "reference": codeterm_types::message_markers::voice(&stored[0], Some("hello"), None),
        })),
    )
    .unwrap();
    assert_eq!(voice.kind, AttachmentKind::Voice);
    assert!(voice.reference.as_deref().unwrap().contains("hello"));
    assert!(!voice.reference.unwrap().contains(&voice_source_str));

    let image_source_str = image_source.to_string_lossy().to_string();
    let image = parse_ingest_response(
        &image_source_str,
        ok_response(serde_json::json!({ "path": stored[1], "kind": "file", "size": 10 })),
    )
    .unwrap();
    assert_eq!(image.reference, None);
    assert_ne!(image.path, voice.path);

    let _ = std::fs::remove_dir_all(origin);
    let _ = std::fs::remove_dir_all(target);
}

#[test]
fn mesh_attachment_transfer_refuses_unusable_sources() {
    let origin = scratch("unusable");
    let empty = origin.join("empty.png");
    std::fs::write(&empty, b"").unwrap();
    for source in [origin.join("missing.png"), origin.clone(), empty] {
        assert!(build_ingest_request(&source, AttachmentKind::File).is_err());
    }
    let _ = std::fs::remove_dir_all(origin);
}

#[test]
fn mesh_attachment_transfer_failure_never_yields_a_reference() {
    let source = "C:\\Users\\blackstar\\.codeterm\\uploads\\1a2b-note.webm";
    let refused = RelayResponse {
        ok: false,
        status: 413,
        data: None,
        error: Some("attachment exceeds limit".into()),
    };
    let error = parse_ingest_response(source, refused).unwrap_err();
    assert!(error.contains("attachment exceeds limit"), "{error}");

    let silent = RelayResponse {
        ok: true,
        status: 200,
        data: None,
        error: None,
    };
    assert!(parse_ingest_response(source, silent).is_err());
    assert!(parse_ingest_response(source, ok_response(serde_json::json!({"nope": 1}))).is_err());
    assert!(parse_ingest_response(
        source,
        ok_response(serde_json::json!({ "path": source, "kind": "voice", "size": 1 }))
    )
    .is_err());
    assert!(parse_ingest_response(
        source,
        ok_response(serde_json::json!({
            "path": "/Users/air/.codeterm/uploads/ffff-note.webm",
            "kind": "voice",
            "size": 1,
            "reference": format!("marker file={source}"),
        }))
    )
    .is_err());
}

#[test]
fn mesh_attachment_path_guard_rejects_unrewritten_source_paths() {
    let root = scratch("guard-root");
    let roots = vec![root.to_string_lossy().to_string()];
    let voice = root.join("ab12cd34-voice note.webm");
    let image = root.join("cd34ef56-photo.png");
    std::fs::write(&voice, b"v").unwrap();
    std::fs::write(&image, b"i").unwrap();
    let voice = voice.to_string_lossy().to_string();
    let image = image.to_string_lossy().to_string();

    let texts = [
        format!("see {image}"),
        format!("see '{voice}' please"),
        codeterm_types::message_markers::voice(&voice, Some("hello"), None),
        format!("see {}", image.replace('\\', "/")),
    ];
    for text in &texts {
        for path in ["/api/tabs/t1/chat-send", "/api/tabs/t1/send", "/api/tabs/t1/delayed-messages"] {
            assert_eq!(
                guard_remote_send("POST", path, &send_body(text), &roots, &host_is_file),
                Err(SOURCE_PATH_REFUSAL.to_string()),
                "{path}: {text}"
            );
        }
    }
    let batch = serde_json::json!({ "batch": [{ "text": "hi" }, { "text": format!("x {image}") }] });
    assert!(guard_remote_send("POST", "/api/tabs/t1/send", &batch, &roots, &host_is_file).is_err());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mesh_attachment_path_guard_allows_rewritten_references() {
    let root = scratch("guard-rewritten");
    let roots = vec![root.to_string_lossy().to_string()];
    let source = root.join("ab12cd34-photo.png");
    std::fs::write(&source, b"i").unwrap();
    let same_layout_target = root.join("9f8e7d6c-ab12cd34-photo.png");
    let target_text = format!("see {}", same_layout_target.to_string_lossy());
    let source_text = format!("see {}", source.to_string_lossy());

    assert!(guard_remote_send(
        "POST",
        "/api/tabs/t1/chat-send",
        &send_body(&target_text),
        &roots,
        &host_is_file
    )
    .is_ok());
    assert!(guard_remote_send(
        "POST",
        "/api/tabs/t1/chat-send",
        &send_body("plain text"),
        &roots,
        &host_is_file
    )
    .is_ok());
    for (method, path) in [("GET", "/api/tabs/t1/output"), ("POST", "/api/tabs/t1/rename")] {
        assert!(
            guard_remote_send(method, path, &send_body(&source_text), &roots, &host_is_file)
                .is_ok()
        );
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mesh_attachment_path_guard_covers_host_attachment_roots() {
    let roots = source_attachment_roots();
    let uploads = crate::api::web_routes::uploads_dir();
    assert!(roots.contains(&uploads.to_string_lossy().to_string()));
    let bucket = crate::attachments::bucket_dir("chat")
        .to_string_lossy()
        .to_string();
    assert!(roots.iter().any(|root| bucket.starts_with(root.as_str())));
}

#[test]
fn mesh_relay_attachment_route_is_owner_checked() {
    let command = crate::api::mesh_routes::remote_tabs::route_remote_tab_action(
        "mesh:air/tab-7",
        "air",
        true,
        "POST",
        MESH_ATTACHMENT_SUFFIX,
    )
    .unwrap();
    assert_eq!(command.path, "/api/tabs/tab-7/attachments");
    assert_eq!(command.delivery_id, None);
    assert!(is_attachment_ingest(&command.method, &command.path));

    for (peer, connected) in [("blackstar", true), ("air", false)] {
        assert!(crate::api::mesh_routes::remote_tabs::route_remote_tab_action(
            "mesh:air/tab-7",
            peer,
            connected,
            "POST",
            MESH_ATTACHMENT_SUFFIX,
        )
        .is_err());
    }
}

#[test]
fn mesh_relay_attachment_body_limit_covers_encoded_maximum() {
    let envelope = serde_json::to_vec(&payload(AttachmentKind::Voice, "voice-note.webm", b""))
        .unwrap()
        .len();
    assert!(MESH_ATTACHMENT_RELAY_BODY_LIMIT >= ENCODED_MAX_BYTES + envelope + 512);
    assert!(ENCODED_MAX_BYTES as u64 >= MESH_ATTACHMENT_MAX_BYTES / 3 * 4);
}

#[test]
fn mesh_relay_attachment_ingest_outlives_transcription() {
    let deadline = crate::api::mesh_routes::relay::local_forward_deadline;
    assert_eq!(
        deadline("POST", "/api/tabs/t1/attachments"),
        deadline("POST", "/api/tabs/t1/send")
    );
    assert!(deadline("POST", "/api/tabs/t1/attachments") > deadline("GET", "/api/tabs/t1/output"));
}

#[test]
fn mesh_relay_attachment_requires_authenticated_peer_access() {
    use crate::api::access_policy::check_access;
    use crate::api::auth::{is_custom_auth, PUBLIC_PATHS};

    for path in ["/api/tabs/t1/attachments", "/api/mesh/attachments/transfer"] {
        assert!(!is_custom_auth(&Method::POST, path), "{path}");
        assert!(!PUBLIC_PATHS.contains(&path), "{path}");
    }
    let ingest = "/api/tabs/t1/attachments";
    assert!(check_access(&AccessLevel::Full, &Method::POST, ingest));
    assert!(check_access(&AccessLevel::AgentIsolated, &Method::POST, ingest));
    assert!(!check_access(&AccessLevel::ReadOnly, &Method::POST, ingest));
    assert!(check_access(
        &AccessLevel::Custom(vec![Permission::SendInput]),
        &Method::POST,
        ingest
    ));
    assert!(!check_access(
        &AccessLevel::Custom(vec![Permission::ReadTabs]),
        &Method::POST,
        ingest
    ));
}
