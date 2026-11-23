use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};

pub const MAX_SVG_BYTES: usize = 8 * 1024;
pub const MAX_CHAT_TILE_BYTES: usize = 64 * 1024;

mod gradient;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SvgUse {
    PluginIcon,
    ChatTile,
}

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

pub fn sanitize_svg(bytes: &[u8], usage: SvgUse) -> Option<String> {
    let max_bytes = match usage {
        SvgUse::PluginIcon => MAX_SVG_BYTES,
        SvgUse::ChatTile => MAX_CHAT_TILE_BYTES,
    };
    if bytes.is_empty() || bytes.len() > max_bytes {
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
                let output = sanitized_element(&start, &name, usage, &mut references)?;
                writer.write_event(Event::Start(output)).ok()?;
                saw_root = true;
                if usage == SvgUse::ChatTile && stack.len() >= 32 {
                    return None;
                }
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
                let output = sanitized_element(&start, &name, usage, &mut references)?;
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
                return (output.len() <= max_bytes)
                    .then(|| String::from_utf8(output).ok())
                    .flatten();
            }
            Event::CData(_) | Event::Comment(_) | Event::DocType(_) | Event::PI(_) => return None,
        }
        buffer.clear();
    }
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
    usage: SvgUse,
    references: &mut gradient::References,
) -> Option<BytesStart<'static>> {
    let mut output = BytesStart::new(name.to_string());
    if usage == SvgUse::ChatTile && gradient::definition(name) {
        return None;
    }
    let mut has_view_box = false;
    let mut has_namespace = false;
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.ok()?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .ok()?
            .to_string();
        let mut value = attribute.unescape_value().ok()?.into_owned();
        if usage == SvgUse::ChatTile
            && matches!(key.as_str(), "d" | "points" | "viewBox" | "transform")
        {
            value = value.replace(['\t', '\r', '\n'], " ");
        }
        let valid = match (usage, key.as_str()) {
            (SvgUse::ChatTile, "transform") => gradient::transform(&value),
            (SvgUse::ChatTile, "stroke-dasharray") => valid_dash_array(&value),
            _ => {
                allowed_attribute(name, &key)
                    && if gradient::definition(name) {
                        gradient::attribute_value(&key, &value)
                    } else {
                        allowed_attribute_value(&key, &value)
                    }
            }
        };
        if !valid {
            return None;
        }
        if usage == SvgUse::ChatTile {
            if value.to_ascii_lowercase().contains("url(") {
                return None;
            }
            if name == "svg" && matches!(key.as_str(), "width" | "height") {
                continue;
            }
            has_view_box |= key == "viewBox";
            has_namespace |= key == "xmlns";
        }
        references.observe(&key, &value)?;
        output.push_attribute((key.as_str(), value.as_str()));
    }
    if usage == SvgUse::ChatTile && name == "svg" {
        if !has_view_box {
            return None;
        }
        if !has_namespace {
            output.push_attribute(("xmlns", "http://www.w3.org/2000/svg"));
        }
    }
    Some(output)
}

fn allowed_attribute(element: &str, attribute: &str) -> bool {
    if gradient::definition(element) {
        return gradient::attribute(element, attribute);
    }
    match attribute {
        "fill" | "stroke" | "fill-rule" | "clip-rule" | "opacity" | "fill-opacity"
        | "stroke-opacity" | "stroke-width" | "stroke-linecap" | "stroke-linejoin"
        | "stroke-miterlimit" => true,
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
        "fill-rule" | "clip-rule" => matches!(value, "evenodd" | "nonzero"),
        "stroke-linecap" => matches!(value, "butt" | "round" | "square"),
        "stroke-linejoin" => matches!(value, "miter" | "round" | "bevel"),
        "opacity" | "fill-opacity" | "stroke-opacity" => unit_interval(value),
        "stroke-width" | "stroke-miterlimit" | "r" | "rx" | "ry" | "width" | "height" => {
            nonnegative_number(value)
        }
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

fn valid_dash_array(value: &str) -> bool {
    let mut count = 0;
    let mut positive = false;
    for part in value.split([',', ' ']).filter(|part| !part.is_empty()) {
        let Ok(length) = part.parse::<f64>() else {
            return false;
        };
        count += 1;
        if count > 32 || !(length == 0.0 || (0.25..=4096.0).contains(&length)) {
            return false;
        }
        positive |= length > 0.0;
    }
    positive
}

fn nonnegative_number(value: &str) -> bool {
    value
        .parse::<f64>()
        .ok()
        .is_some_and(|v| v.is_finite() && v >= 0.0)
}

fn unit_interval(value: &str) -> bool {
    value
        .parse::<f64>()
        .ok()
        .is_some_and(|v| v.is_finite() && (0.0..=1.0).contains(&v))
}
