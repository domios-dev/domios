use std::io::Read;
use std::path::{Component, Path};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use quick_xml::Writer;

pub const MAX_PLUGIN_ICON_BYTES: usize = 8 * 1024;

const ALLOWED_ELEMENTS: &[&str] = &[
    "svg", "g", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon",
];

pub fn load_plugin_icon(bundle_dir: &Path, relative_path: &str) -> Option<String> {
    if !valid_relative_svg_path(relative_path) {
        return None;
    }
    let root = std::fs::canonicalize(bundle_dir).ok()?;
    let asset = std::fs::canonicalize(root.join(relative_path)).ok()?;
    if !asset.starts_with(&root) || !asset.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(asset).ok()?;
    let mut bytes = Vec::with_capacity(MAX_PLUGIN_ICON_BYTES + 1);
    file.by_ref()
        .take((MAX_PLUGIN_ICON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_PLUGIN_ICON_BYTES {
        return None;
    }
    sanitize_plugin_icon_svg(&bytes)
}

/// Accept only `#rgb[a]`/`#rrggbb[aa]` or a CodeTerm theme token, so a manifest color can never carry CSS syntax.
pub fn sanitize_plugin_icon_color(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let valid =
            matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
        return valid.then(|| value.to_string());
    }
    let token = value.strip_prefix("var(--ct-")?.strip_suffix(')')?;
    let valid = !token.is_empty()
        && token.len() <= 48
        && token
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    valid.then(|| value.to_string())
}

pub fn sanitize_plugin_icon_svg(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_PLUGIN_ICON_BYTES {
        return None;
    }

    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut buffer = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut saw_root = false;
    let mut closed_root = false;

    loop {
        let event = reader.read_event_into(&mut buffer).ok()?;
        match event {
            Event::Decl(_) => {
                if saw_root {
                    return None;
                }
            }
            Event::Start(start) => {
                if closed_root {
                    return None;
                }
                let name = element_name(&start)?;
                if !allowed_element(&name) || (stack.is_empty() && name != "svg") {
                    return None;
                }
                let output = sanitized_element(&start, &name)?;
                writer.write_event(Event::Start(output)).ok()?;
                saw_root = true;
                stack.push(name);
            }
            Event::Empty(start) => {
                if closed_root {
                    return None;
                }
                let name = element_name(&start)?;
                if !allowed_element(&name) || (stack.is_empty() && name != "svg") {
                    return None;
                }
                let output = sanitized_element(&start, &name)?;
                writer.write_event(Event::Empty(output)).ok()?;
                if stack.is_empty() {
                    saw_root = true;
                    closed_root = true;
                }
            }
            Event::End(end) => {
                let end_name = end.name();
                let name = std::str::from_utf8(end_name.as_ref()).ok()?;
                if stack.pop().as_deref() != Some(name) {
                    return None;
                }
                writer.write_event(Event::End(end.into_owned())).ok()?;
                if stack.is_empty() {
                    closed_root = true;
                }
            }
            Event::Text(text) => {
                if !text.unescape().ok()?.chars().all(char::is_whitespace) {
                    return None;
                }
            }
            Event::Eof => {
                if !saw_root || !closed_root || !stack.is_empty() {
                    return None;
                }
                let output = writer.into_inner();
                return (output.len() <= MAX_PLUGIN_ICON_BYTES)
                    .then(|| String::from_utf8(output).ok())
                    .flatten();
            }
            Event::CData(_) | Event::Comment(_) | Event::DocType(_) | Event::PI(_) => return None,
        }
        buffer.clear();
    }
}

fn valid_relative_svg_path(path: &str) -> bool {
    !path.is_empty()
        && path == path.trim()
        && path.to_ascii_lowercase().ends_with(".svg")
        && !path.contains('\0')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.contains("://")
        && !path.to_ascii_lowercase().starts_with("data:")
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn element_name(element: &BytesStart<'_>) -> Option<String> {
    Some(
        std::str::from_utf8(element.name().as_ref())
            .ok()?
            .to_string(),
    )
}

fn allowed_element(name: &str) -> bool {
    ALLOWED_ELEMENTS.contains(&name)
}

fn sanitized_element(element: &BytesStart<'_>, name: &str) -> Option<BytesStart<'static>> {
    let mut output = BytesStart::new(name.to_string());
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.ok()?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .ok()?
            .to_string();
        let value = attribute.unescape_value().ok()?.into_owned();
        if !allowed_attribute(name, &key) || !allowed_attribute_value(&key, &value) {
            return None;
        }
        output.push_attribute((key.as_str(), value.as_str()));
    }
    Some(output)
}

fn allowed_attribute(element: &str, attribute: &str) -> bool {
    match attribute {
        "fill" | "stroke" | "stroke-width" | "stroke-linecap" | "stroke-linejoin" => true,
        "viewBox" | "xmlns" => element == "svg",
        "d" => element == "path",
        "cx" | "cy" => matches!(element, "circle" | "ellipse"),
        "r" => element == "circle",
        "rx" | "ry" => matches!(element, "rect" | "ellipse"),
        "x" | "y" | "width" | "height" => element == "rect",
        "x1" | "y1" | "x2" | "y2" => element == "line",
        "points" => matches!(element, "polyline" | "polygon"),
        _ => false,
    }
}

fn allowed_attribute_value(attribute: &str, value: &str) -> bool {
    if attribute == "xmlns" {
        return value == "http://www.w3.org/2000/svg";
    }
    if value.is_empty()
        || value.chars().any(char::is_control)
        || value.contains('#')
        || value.to_ascii_lowercase().contains("url(")
        || value.contains("://")
        || value.to_ascii_lowercase().starts_with("data:")
    {
        return false;
    }
    match attribute {
        "fill" | "stroke" => matches!(value, "currentColor" | "none"),
        "stroke-linecap" => matches!(value, "butt" | "round" | "square"),
        "stroke-linejoin" => matches!(value, "miter" | "round" | "bevel"),
        "stroke-width" | "r" | "rx" | "ry" | "width" | "height" => nonnegative_number(value),
        "cx" | "cy" | "x" | "y" | "x1" | "y1" | "x2" | "y2" => finite_number(value),
        "points" => value
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .all(finite_number),
        "viewBox" => {
            let values: Vec<_> = value.split_whitespace().collect();
            values.len() == 4
                && values.iter().all(|value| finite_number(value))
                && values[2].parse::<f64>().ok().is_some_and(|v| v > 0.0)
                && values[3].parse::<f64>().ok().is_some_and(|v| v > 0.0)
        }
        _ => true,
    }
}

fn finite_number(value: &str) -> bool {
    value.parse::<f64>().ok().is_some_and(f64::is_finite)
}

fn nonnegative_number(value: &str) -> bool {
    value
        .parse::<f64>()
        .ok()
        .is_some_and(|v| v.is_finite() && v >= 0.0)
}

#[cfg(test)]
mod tests {
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
