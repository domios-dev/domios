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
        "viewBox" => element == "svg",
        "d" => element == "path",
        "cx" | "cy" | "r" => element == "circle",
        _ => false,
    }
}

fn allowed_attribute_value(attribute: &str, value: &str) -> bool {
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
        "stroke-width" => nonnegative_number(value),
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
            br#"<svg><rect width="1"/></svg>"#.as_slice(),
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
