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
