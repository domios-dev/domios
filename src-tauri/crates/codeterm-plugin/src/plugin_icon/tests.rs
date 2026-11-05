use super::*;
use std::path::PathBuf;

const VALID: &[u8] = br#"<svg viewBox="0 0 24 24" fill="currentColor"><g stroke="none"><path d="M2 2 L22 22"/></g><circle cx="12" cy="12" r="4" fill="none"/></svg>"#;

#[test]
fn plugin_icon_svg_sanitizes_allowlisted_current_color_asset() {
    let output = sanitize_plugin_icon_svg(VALID).expect("valid icon");
    assert!(output.contains("currentColor"));
    assert!(output.contains("<path"));
    assert!(!output.contains("xmlns"));
}

#[test]
fn plugin_icon_svg_rejects_active_or_external_content() {
    for source in [
        br#"<svg><script>alert(1)</script></svg>"#.as_slice(),
        br#"<svg><foreignObject><div/></foreignObject></svg>"#.as_slice(),
        br#"<svg><path onclick="alert(1)" d="M0 0"/></svg>"#.as_slice(),
        br#"<svg><path href="https://example.test/icon" d="M0 0"/></svg>"#.as_slice(),
        br#"<svg><path xlink:href="https://example.test/icon" d="M0 0"/></svg>"#.as_slice(),
        br#"<svg><style>*{fill:red}</style></svg>"#.as_slice(),
        br#"<svg><path fill="url(#paint)" d="M0 0"/></svg>"#.as_slice(),
        br#"<svg><rect transform="scale(2)" width="1"/></svg>"#.as_slice(),
    ] {
        assert!(
            sanitize_plugin_icon_svg(source).is_none(),
            "accepted {source:?}"
        );
    }
}

#[test]
fn plugin_icon_svg_rejects_malformed_or_oversized_assets() {
    assert!(sanitize_plugin_icon_svg(br#"<svg><path/></svg>tail"#).is_none());
    assert!(sanitize_plugin_icon_svg(&vec![b'x'; MAX_PLUGIN_ICON_BYTES + 1]).is_none());
}

#[test]
fn plugin_icon_svg_loads_only_a_bounded_relative_bundle_file() {
    let root = std::env::temp_dir().join(format!("ct-plugin-icon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("assets")).expect("bundle");
    std::fs::write(root.join("assets/icon.svg"), VALID).expect("asset");
    assert_eq!(
        load_plugin_icon(&root, "assets/icon.svg"),
        sanitize_plugin_icon_svg(VALID)
    );
    assert!(load_plugin_icon(&root, "assets/missing.svg").is_none());
    assert!(load_plugin_icon(&root, "../icon.svg").is_none());
    assert!(load_plugin_icon(&root, "https://example.test/icon.svg").is_none());
    assert!(load_plugin_icon(&root, "data:image/svg+xml,icon.svg").is_none());
    assert!(load_plugin_icon(&root, "stream:icon.svg").is_none());
    std::fs::write(
        root.join("assets/large.svg"),
        vec![b'x'; MAX_PLUGIN_ICON_BYTES + 1],
    )
    .expect("large asset");
    assert!(load_plugin_icon(&root, "assets/large.svg").is_none());
    let _ = std::fs::remove_dir_all(PathBuf::from(root));
}

#[cfg(test)]
mod legacy_shape_tests {
    use super::sanitize_plugin_icon_svg;

    #[test]
    fn plugin_icon_svg_accepts_legacy_channel_icon_shapes() {
        let namespaced = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M4 4h16v16H4z"/></svg>"##;
        assert!(sanitize_plugin_icon_svg(namespaced.as_bytes()).is_some());
        let geometry = r##"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="14" rx="2"/><line x1="8" y1="20" x2="16" y2="20"/><ellipse cx="12" cy="11" rx="3" ry="2"/><polyline points="4,4 8,8 12,4"/></svg>"##;
        assert!(sanitize_plugin_icon_svg(geometry.as_bytes()).is_some());
    }

    #[test]
    fn plugin_icon_svg_rejects_unsafe_geometry_and_foreign_namespaces() {
        let foreign =
            r##"<svg xmlns="https://evil.example/svg" viewBox="0 0 1 1"><path d="M0 0"/></svg>"##;
        assert!(sanitize_plugin_icon_svg(foreign.as_bytes()).is_none());
        let negative =
            r##"<svg viewBox="0 0 1 1"><rect x="0" y="0" width="-1" height="1"/></svg>"##;
        assert!(sanitize_plugin_icon_svg(negative.as_bytes()).is_none());
        let points = r##"<svg viewBox="0 0 1 1"><polygon points="0,0 url(x)"/></svg>"##;
        assert!(sanitize_plugin_icon_svg(points.as_bytes()).is_none());
    }
}

#[cfg(test)]
mod icon_color_tests {
    use super::sanitize_plugin_icon_color;

    #[test]
    fn plugin_icon_color_accepts_hex_and_theme_tokens() {
        for color in [
            "#d97757",
            "#fff",
            "#ffff",
            "#175ddc80",
            "var(--ct-fg)",
            "var(--ct-accent-2)",
        ] {
            assert_eq!(sanitize_plugin_icon_color(color).as_deref(), Some(color));
        }
    }

    #[test]
    fn plugin_icon_color_rejects_css_syntax() {
        for color in [
            "",
            "red",
            "#12",
            "#ggg",
            "url(x)",
            "var(--other)",
            "var(--ct-fg); background: red",
            "var(--ct-FG)",
            "var(--ct-)",
            "expression(alert(1))",
        ] {
            assert!(sanitize_plugin_icon_color(color).is_none(), "{color}");
        }
    }
}

#[test]
fn plugin_icon_svg_accepts_shipped_color_marks_and_local_gradients() {
    for bytes in [
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-acp/gemini-color.svg"
        ))
        .as_slice(),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-acp/deepseek-color.svg"
        ))
        .as_slice(),
    ] {
        assert!(bytes.len() <= MAX_PLUGIN_ICON_BYTES);
        assert!(sanitize_plugin_icon_svg(bytes).is_some());
    }
    let radial = br##"<svg viewBox="0 0 24 24"><defs><radialGradient id="paint" cx="50%" cy="50%" r="50%" fx="0" fy="1" gradientTransform="translate(1,2) scale(2)" gradientUnits="objectBoundingBox"><stop offset="0%" stop-color="#fff" stop-opacity="1"/><stop offset="100%" stop-color="#000"/></radialGradient></defs><path fill="url(#paint)" stroke="url(#paint)" fill-rule="evenodd" d="M0 0"/></svg>"##;
    assert!(sanitize_plugin_icon_svg(radial).is_some());
}

#[test]
fn plugin_icon_svg_refuses_external_missing_duplicate_and_inherited_gradients() {
    for svg in [
        r##"<svg><path fill="url(https://example.org/a.svg#paint)"/></svg>"##,
        r##"<svg><path fill="url(#missing)"/></svg>"##,
        r##"<svg><defs><linearGradient id="paint" href="#other"/></defs><path fill="url(#paint)"/></svg>"##,
        r##"<svg><defs><linearGradient id="paint" xlink:href="https://example.org/a"/></defs></svg>"##,
        r##"<svg><defs><linearGradient id="paint"/><radialGradient id="paint"/></defs></svg>"##,
        r##"<svg><linearGradient id="paint"/></svg>"##,
        r##"<svg><stop stop-color="#fff"/></svg>"##,
        r##"<svg><defs><linearGradient id="paint"><stop stop-color="url(#paint)"/></linearGradient></defs></svg>"##,
        r##"<svg><defs><linearGradient id="paint" gradientTransform="url(https://example.org/a)"/></defs></svg>"##,
        r##"<svg><defs><linearGradient id="paint"><stop stop-color="#fff" stop-opacity="2"/></linearGradient></defs></svg>"##,
        r##"<svg><path fill-rule="url(#paint)"/></svg>"##,
    ] {
        assert!(sanitize_plugin_icon_svg(svg.as_bytes()).is_none(), "{svg}");
    }
}
