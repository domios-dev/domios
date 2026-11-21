use super::*;

#[test]
fn registry_svg_preserves_sized_current_color_path_asset() {
    let bytes = include_bytes!("fixtures/registry-current-color.svg");
    let icon = sanitize("image/svg+xml", bytes).expect("sized registry SVG");
    assert!(icon.contains("width=\"16\""));
    assert!(icon.contains("height=\"16\""));
    assert!(icon.contains("viewBox=\"0 0 1200 1200\""));
    assert!(icon.contains("fill=\"currentColor\""));
    assert_eq!(icon.matches("<path ").count(), 1);
    assert_eq!(
        sanitize("image/svg+xml", icon.as_bytes()).as_deref(),
        Some(icon.as_str())
    );
    for dimensions in [
        "width=\"-1\"",
        "height=\"NaN\"",
        "width=\"inf\"",
        "height=\"url(x)\"",
    ] {
        let source = format!("<svg {dimensions}><path d=\"M0 0\"/></svg>");
        assert!(sanitize("image/svg+xml", source.as_bytes()).is_none());
    }
}

#[test]
fn refuses_insecure_urls_types_sizes_and_active_svg() {
    for url in [
        "http://example.org/a",
        "file:///a",
        "https://user@example.org/a",
        "javascript:1",
    ] {
        assert!(!valid_url(url), "{url}");
    }
    assert!(valid_url("https://example.org/a.png?q=1"));
    assert!(sanitize("text/html", b"<svg/>").is_none());
    assert!(sanitize("image/png", b"<svg/>").is_none());
    assert!(sanitize("image/png", &vec![0; MAX_BYTES + 1]).is_none());
    for svg in [
        "<svg><script/></svg>",
        "<svg><foreignObject/></svg>",
        "<svg><image href='https://example.org/x'/></svg>",
        "<svg onload='alert(1)'/>",
        "<!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///a'>]><svg>&x;</svg>",
    ] {
        assert!(sanitize("image/svg+xml", svg.as_bytes()).is_none(), "{svg}");
    }
    assert!(sanitize(
        "image/svg+xml",
        b"<svg viewBox='0 0 24 24'><path d='M0 0L1 1'/></svg>"
    )
    .is_some());
}

#[test]
fn cache_is_revalidated_and_expires_without_network() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("icon.json");
    let entry = Cached {
        timestamp: 100,
        mime: "image/svg+xml".into(),
        body: STANDARD.encode(b"<svg viewBox='0 0 24 24'><path d='M0 0'/></svg>"),
    };
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    assert!(cached(&path, Some(101)).is_some());
    assert!(cached(&path, Some(99)).is_none());
    assert!(cached(&path, Some(100 + TTL.as_secs())).is_none());
    let entry = Cached {
        body: STANDARD.encode(b"<svg><script/></svg>"),
        ..entry
    };
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    assert!(cached(&path, Some(101)).is_none());
}

#[test]
fn sleeping_profile_display_revalidates_aged_url_keyed_icons_without_network() {
    let root = tempfile::tempdir().unwrap();
    let url = "https://example.org/profile.svg";
    let path = root
        .path()
        .join(format!("{:x}.json", Sha256::digest(url.as_bytes())));
    let entry = Cached {
        timestamp: 0,
        mime: "image/svg+xml".into(),
        body: STANDARD.encode(b"<svg viewBox='0 0 24 24'><path d='M1 1'/></svg>"),
    };
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    assert!(cached(&path, Some(TTL.as_secs() + 1)).is_none());
    assert!(read_cached(root.path(), url).is_some());
    assert!(read_cached(root.path(), "https://example.org/other.svg").is_none());
    assert!(read_cached(root.path(), "http://example.org/profile.svg").is_none());
    let invalid = Cached {
        body: STANDARD.encode(b"<svg><script/></svg>"),
        ..entry
    };
    std::fs::write(path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(read_cached(root.path(), url).is_none());
}

#[test]
fn raster_types_need_matching_signatures_and_use_data_only() {
    for (mime, bytes) in [
        ("image/png", b"\x89PNG\r\n\x1a\n".as_slice()),
        ("image/jpeg", b"\xff\xd8\xff"),
        ("image/webp", b"RIFF0000WEBP"),
    ] {
        let icon = sanitize(mime, bytes).unwrap();
        assert!(icon.contains(&format!("href=\"data:{mime};base64,")));
    }
    assert!(sanitize("image/webp", b"RIFF").is_none());
}

#[test]
fn redirect_policy_and_mime_caps_bound_https_svg_fetches() {
    assert!(redirect_allowed("https://cdn.example.test/icon.svg", 0));
    assert!(redirect_allowed("https://cdn.example.test/icon.svg", MAX_REDIRECTS - 1));
    assert!(!redirect_allowed("https://cdn.example.test/icon.svg", MAX_REDIRECTS));
    assert!(!redirect_allowed("http://cdn.example.test/icon.svg", 0));
    assert!(!redirect_allowed("file:///tmp/icon.svg", 0));
    assert!(!redirect_allowed("https://user@cdn.example.test/icon.svg", 0));
    assert_eq!(
        max_bytes_for_mime("image/svg+xml; charset=utf-8"),
        Some(crate::plugin_icon::MAX_PLUGIN_ICON_BYTES)
    );
    assert_eq!(max_bytes_for_mime("image/png"), Some(MAX_BYTES));
    assert!(max_bytes_for_mime("text/html").is_none());
    assert!(max_bytes_for_mime("application/javascript").is_none());
}

#[tokio::test]
async fn resolve_caches_valid_svg_rejects_bad_payloads_and_falls_back_offline() {
    let root = tempfile::tempdir().unwrap();
    let url = "https://cdn.example.test/brand.svg";
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M1 1h22v22H1z\"/></svg>";

    let fetched = resolve_with(root.path().to_path_buf(), url, 1_000, |_| async {
        FetchOutcome::Ok {
            mime: "image/svg+xml; charset=utf-8".into(),
            bytes: svg.to_vec(),
        }
    })
    .await
    .expect("valid fetched SVG");
    assert!(fetched.contains("<path"));
    assert_eq!(read_cached(root.path(), url).as_deref(), Some(fetched.as_str()));

    let mut called = false;
    let cached_hit = resolve_with(root.path().to_path_buf(), url, 1_001, |_| {
        called = true;
        async { FetchOutcome::Unavailable }
    })
    .await;
    assert!(!called, "fresh cache must not hit network");
    assert_eq!(cached_hit.as_deref(), Some(fetched.as_str()));

    let offline = resolve_with(
        root.path().to_path_buf(),
        url,
        1_000 + TTL.as_secs() + 10,
        |_| async { FetchOutcome::Unavailable },
    )
    .await;
    assert_eq!(
        offline.as_deref(),
        Some(fetched.as_str()),
        "offline refresh reuses revalidated cached icon"
    );

    let bad_url = "https://cdn.example.test/malicious.svg";
    for bad in [
        b"<svg><script>alert(1)</script></svg>".to_vec(),
        b"<svg><mask id=\"m\"/><foreignObject/></svg>".to_vec(),
        vec![b'a'; crate::plugin_icon::MAX_PLUGIN_ICON_BYTES + 1],
    ] {
        assert!(resolve_with(root.path().to_path_buf(), bad_url, 2_000, |_| async {
            FetchOutcome::Ok {
                mime: "image/svg+xml".into(),
                bytes: bad,
            }
        })
        .await
        .is_none());
        assert!(read_cached(root.path(), bad_url).is_none());
    }

    let missing_url = "https://cdn.example.test/offline.svg";
    assert!(
        resolve_with(root.path().to_path_buf(), missing_url, 2_000, |_| async {
            FetchOutcome::Unavailable
        })
        .await
        .is_none()
    );
}
