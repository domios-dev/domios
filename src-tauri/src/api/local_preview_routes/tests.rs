use super::*;
use axum::body::to_bytes;

mod fixture;
use fixture::{directory_link, Fixture};

async fn json(response: Response) -> serde_json::Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 12 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

fn assert_headers(response: &Response) {
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        response.headers()[header::CONTENT_SECURITY_POLICY],
        "default-src 'none'; sandbox; frame-ancestors 'none'"
    );
}

#[tokio::test]
async fn authentication_is_required_on_read_and_batch_routes() {
    let fixture = Fixture::new();
    for suffix in ["read", "check"] {
        for auth in [None, Some("Bearer invalid-token")] {
            let response = fixture
                .post(&fixture.owner, suffix, serde_json::json!({}), auth)
                .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_headers(&response);
        }
    }
}

#[tokio::test]
async fn referenced_images_documents_and_pages_match_desktop_shape_and_headers() {
    let fixture = Fixture::new();
    for (name, bytes, kind) in [
        ("read.md", "# Preview", "document"),
        ("pixel.png", "opaque-image", "image"),
        ("view.html", "<main>preview</main>", "page"),
    ] {
        let path = fixture.write(name, bytes);
        fixture.reference(&fixture.owner, &path);
        let response = fixture.read(&fixture.owner, &path, kind).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_headers(&response);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let desktop = crate::commands::html_preview::read_local_preview_file_guarded(
            path.to_str().unwrap(),
            kind,
            |_, _| Ok(()),
        )
        .unwrap();
        assert_eq!(json(response).await, serde_json::to_value(desktop).unwrap());
    }
}

#[tokio::test]
async fn unreferenced_and_other_tabs_paths_return_identical_not_found() {
    let fixture = Fixture::new();
    let path = fixture.write("owned.md", "opaque");
    fixture.reference(&fixture.owner, &path);
    let own = fixture.read(&fixture.owner, &path, "document").await;
    assert_eq!(own.status(), StatusCode::OK);
    let foreign = fixture.read(&fixture.sibling, &path, "document").await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let missing = fixture
        .read(
            &fixture.sibling,
            &fixture.root.join("missing.md"),
            "document",
        )
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(foreign).await, json(missing).await);
}

#[tokio::test]
async fn relative_traversal_network_device_and_ads_paths_are_refused() {
    let fixture = Fixture::new();
    let traversal = fixture.root.join("sub/../read.md");
    let ads = fixture.root.join("read.md:stream");
    fixture.write("read.md", "opaque");
    for raw in [
        "relative.md".to_string(),
        traversal.to_string_lossy().into_owned(),
        ads.to_string_lossy().into_owned(),
        "//server/share/read.md".into(),
        r"\\?\C:\read.md".into(),
        r"\\.\C:\read.md".into(),
    ] {
        fixture.reference(&fixture.owner, std::path::Path::new(&raw));
        let response = fixture
            .post(
                &fixture.owner,
                "read",
                serde_json::json!({"path": raw, "kind": "document"}),
                Some("Bearer preview-test-token"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn symlink_or_junction_escape_is_refused_even_when_referenced() {
    let fixture = Fixture::new();
    let target = fixture.write("target/read.md", "opaque");
    let alias = fixture.root.join("alias");
    directory_link(target.parent().unwrap(), &alias);
    let path = alias.join("read.md");
    fixture.reference(&fixture.owner, &path);
    assert_eq!(
        fixture
            .read(&fixture.owner, &path, "document")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn secret_directories_credentials_and_configured_paths_are_refused() {
    let fixture = Fixture::new();
    for name in [
        ".ssh/key.txt",
        ".codeterm/local.md",
        "credentials.json",
        "api-token.txt",
        "private/report.md",
    ] {
        let path = fixture.write(name, "opaque");
        fixture.reference(&fixture.owner, &path);
        assert_eq!(
            fixture
                .read(&fixture.owner, &path, "document")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn missing_directory_wrong_kind_and_oversized_files_match_desktop_refusals() {
    let fixture = Fixture::new();
    let missing = fixture.root.join("missing.md");
    let directory = fixture.root.join("directory.md");
    std::fs::create_dir(&directory).unwrap();
    let wrong = fixture.write("wrong.bin", [b'a', 0, b'b']);
    let non_utf8 = fixture.write("non-utf8.bin", [0xff, 0xfe]);
    let document = fixture.write("document.md", "opaque");
    let large_document = fixture.write("large.md", vec![b'x'; 1_000_001]);
    let large_page = fixture.write("large.html", vec![b'x'; 200_001]);
    let large_image = fixture.write("large.png", vec![b'x'; 10 * 1024 * 1024 + 1]);
    for (path, kind) in [
        (&missing, "document"),
        (&directory, "document"),
        (&wrong, "document"),
        (&non_utf8, "document"),
        (&document, "image"),
        (&document, "page"),
        (&document, "unknown"),
        (&large_document, "document"),
        (&large_page, "page"),
        (&large_image, "image"),
    ] {
        fixture.reference(&fixture.owner, path);
        assert!(
            crate::commands::html_preview::read_local_preview_file_guarded(
                path.to_str().unwrap(),
                kind,
                |_, _| Ok(()),
            )
            .is_err()
        );
        let response = fixture.read(&fixture.owner, path, kind).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_headers(&response);
        assert_eq!(
            json(response).await,
            serde_json::json!({"error": "Preview unavailable"})
        );
    }
}

#[tokio::test]
async fn batch_check_filters_unreferenced_secret_missing_and_unsupported_paths() {
    let fixture = Fixture::new();
    let allowed = fixture.write("allowed.md", "opaque");
    let allowed_text = fixture.write("text.bin", "opaque");
    let unrelated = fixture.write("unrelated.md", "opaque");
    let secret = fixture.write("credentials.json", "opaque");
    let wrong = fixture.write("wrong.bin", [b'a', 0, b'b']);
    let non_utf8 = fixture.write("non-utf8.bin", [0xff, 0xfe]);
    let binary_document = fixture.write("binary.md", [b'a', 0, b'b']);
    let missing = fixture.root.join("missing.md");
    for path in [
        &allowed,
        &allowed_text,
        &secret,
        &wrong,
        &non_utf8,
        &binary_document,
        &missing,
    ] {
        fixture.reference(&fixture.owner, path);
    }
    let response = fixture
        .post(
            &fixture.owner,
            "check",
            serde_json::json!({"paths": [&allowed, &allowed_text, &unrelated, &secret, &wrong, &non_utf8, &binary_document, &missing]}),
            Some("Bearer preview-test-token"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_headers(&response);
    let result = json(response).await;
    assert_eq!(result["paths"], serde_json::json!([allowed, allowed_text]));
    let response = fixture
        .read(&fixture.owner, &allowed_text, "document")
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["data"], "opaque");
}

#[tokio::test]
async fn admission_queues_a_burst_and_still_bounds_requests() {
    let admission = Arc::new(Admission::default());
    let now = Instant::now();
    let mut permits = Vec::new();
    for _ in 0..4 {
        permits.push(admission.admit(now).await.unwrap());
    }
    assert_eq!(
        admission
            .admit_within(now, Duration::ZERO)
            .await
            .unwrap_err(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let queued = {
        let admission = admission.clone();
        tokio::spawn(async move { admission.admit(now).await.is_ok() })
    };
    drop(permits.pop());
    assert!(
        queued.await.unwrap(),
        "a queued read is admitted once a permit frees"
    );
    drop(permits);
    for _ in 6..REQUESTS_PER_WINDOW {
        drop(admission.admit(now).await.unwrap());
    }
    assert_eq!(
        admission.admit(now).await.unwrap_err(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(admission.admit(now + WINDOW).await.is_ok());
}

#[tokio::test]
async fn signed_isolated_peer_cannot_read_or_check_another_tabs_files() {
    let fixture = Fixture::new();
    let private = fixture.isolated_peer();
    let path = fixture.write("owned.md", "opaque");
    fixture.reference(&fixture.owner, &path);
    fixture.reference(&fixture.sibling, &path);
    for tab in [&fixture.owner, &fixture.sibling] {
        for suffix in ["read", "check"] {
            let body = if suffix == "read" {
                serde_json::json!({"path": path, "kind": "document"})
            } else {
                serde_json::json!({"paths": [path]})
            };
            let auth = format!(
                "Bearer {}",
                crate::mesh::crown::sign_request(&private, "preview-peer").unwrap()
            );
            let response = fixture.post(tab, suffix, body, Some(&auth)).await;
            assert_eq!(
                response.status(),
                if tab == &fixture.owner {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                }
            );
            assert_headers(&response);
        }
    }
}

#[tokio::test]
async fn rate_and_batch_limits_are_enforced_by_the_routes() {
    let now = Arc::new(parking_lot::Mutex::new(Instant::now()));
    let clock = now.clone();
    let fixture = Fixture::with_clock(Arc::new(move || *clock.lock()));
    let oversized = fixture
        .post(
            &fixture.owner,
            "check",
            serde_json::json!({"paths": vec!["/unused.md"; MAX_BATCH + 1]}),
            Some("Bearer preview-test-token"),
        )
        .await;
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    for _ in 0..REQUESTS_PER_WINDOW {
        let response = fixture
            .post(
                &fixture.owner,
                "check",
                serde_json::json!({"paths": []}),
                Some("Bearer preview-test-token"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = fixture
        .post(
            &fixture.owner,
            "check",
            serde_json::json!({"paths": []}),
            Some("Bearer preview-test-token"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_headers(&response);
    *now.lock() += WINDOW;
    let renewed = fixture
        .post(
            &fixture.owner,
            "check",
            serde_json::json!({"paths": []}),
            Some("Bearer preview-test-token"),
        )
        .await;
    assert_eq!(renewed.status(), StatusCode::OK);
}

#[tokio::test]
async fn hardlink_alias_is_refused_even_when_referenced() {
    let fixture = Fixture::new();
    let target = fixture.write("target.md", "opaque");
    let alias = fixture.root.join("alias.md");
    std::fs::hard_link(&target, &alias).unwrap();
    fixture.reference(&fixture.owner, &alias);
    assert_eq!(
        fixture
            .read(&fixture.owner, &alias, "document")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
