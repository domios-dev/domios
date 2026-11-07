use std::io::Read;
use std::path::{Component, Path};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use quick_xml::Writer;

pub const MAX_PLUGIN_ICON_BYTES: usize = 8 * 1024;

mod gradient;
pub mod registry;

const ALLOWED_ELEMENTS: &[&str] = &[
    "svg",
    "g",
    "path",
    "circle",
    "ellipse",
    "rect",
    "line",
    "polyline",
    "polygon",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
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
    let mut references = gradient::References::default();

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
                if !allowed_element(&name)
                    || !gradient::valid_parent(&name, stack.last().map(String::as_str))
                {
                    return None;
                }
                let output = sanitized_element(&start, &name, &mut references)?;
                writer.write_event(Event::Start(output)).ok()?;
                saw_root = true;
                stack.push(name);
            }
            Event::Empty(start) => {
                if closed_root {
                    return None;
                }
                let name = element_name(&start)?;
                if !allowed_element(&name)
                    || !gradient::valid_parent(&name, stack.last().map(String::as_str))
                {
                    return None;
                }
                let output = sanitized_element(&start, &name, &mut references)?;
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
                if !saw_root || !closed_root || !stack.is_empty() || !references.valid() {
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

pub(crate) fn valid_relative_svg_path(path: &str) -> bool {
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

fn sanitized_element(
    element: &BytesStart<'_>,
    name: &str,
    references: &mut gradient::References,
) -> Option<BytesStart<'static>> {
    let mut output = BytesStart::new(name.to_string());
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.ok()?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .ok()?
            .to_string();
        let value = attribute.unescape_value().ok()?.into_owned();
        if !allowed_attribute(name, &key)
            || !(if gradient::definition(name) {
                gradient::attribute_value(&key, &value)
            } else {
                allowed_attribute_value(&key, &value)
            })
        {
            return None;
        }
        references.observe(&key, &value)?;
        output.push_attribute((key.as_str(), value.as_str()));
    }
    Some(output)
}

fn allowed_attribute(element: &str, attribute: &str) -> bool {
    if gradient::definition(element) {
        return gradient::attribute(element, attribute);
    }
    match attribute {
        "fill" | "stroke" | "fill-rule" | "stroke-width" | "stroke-linecap" | "stroke-linejoin" => {
            true
        }
        "viewBox" | "xmlns" => element == "svg",
        "d" => element == "path",
        "cx" | "cy" => matches!(element, "circle" | "ellipse"),
        "r" => element == "circle",
        "rx" | "ry" => matches!(element, "rect" | "ellipse"),
        "x" | "y" => element == "rect",
        "width" | "height" => matches!(element, "svg" | "rect"),
        "x1" | "y1" | "x2" | "y2" => element == "line",
        "points" => matches!(element, "polyline" | "polygon"),
        _ => false,
    }
}

fn allowed_attribute_value(attribute: &str, value: &str) -> bool {
    if matches!(attribute, "fill" | "stroke") {
        return gradient::paint(value);
    }
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
        "fill-rule" => matches!(value, "evenodd" | "nonzero"),
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
mod tests;
