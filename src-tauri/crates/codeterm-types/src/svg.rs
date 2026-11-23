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

#[derive(Debug, PartialEq, Eq)]
pub enum SvgRefusal {
    Empty,
    TooLarge,
    InvalidXml,
    InvalidStructure,
    Script,
    EventHandler,
    ExternalReference,
    Reference,
    Style,
    UnsupportedElement,
    UnsupportedAttribute,
    InvalidAttribute,
    UnsupportedMarkup,
    MissingViewBox,
    InvalidViewBox,
    NonSquare,
    TooDeep,
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
    sanitize(bytes, usage, false).ok()
}

pub fn sanitize_wallpaper_upload(bytes: &[u8]) -> Result<String, SvgRefusal> {
    sanitize(bytes, SvgUse::ChatTile, true)
}

fn sanitize(bytes: &[u8], usage: SvgUse, require_square: bool) -> Result<String, SvgRefusal> {
    let max_bytes = match usage {
        SvgUse::PluginIcon => MAX_SVG_BYTES,
        SvgUse::ChatTile => MAX_CHAT_TILE_BYTES,
    };
    if bytes.is_empty() {
        return Err(SvgRefusal::Empty);
    }
    if bytes.len() > max_bytes {
        return Err(SvgRefusal::TooLarge);
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
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| SvgRefusal::InvalidXml)?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Decl(_) => {
                if saw_root {
                    return Err(SvgRefusal::InvalidStructure);
                }
            }
            Event::Start(start) | Event::Empty(start) => {
                if closed_root {
                    return Err(SvgRefusal::InvalidStructure);
                }
                let qualified = start.name();
                let name =
                    std::str::from_utf8(qualified.as_ref()).map_err(|_| SvgRefusal::InvalidXml)?;
                if !ALLOWED_ELEMENTS.contains(&name) {
                    return Err(match name.rsplit(':').next() {
                        Some("script") => SvgRefusal::Script,
                        Some("style") => SvgRefusal::Style,
                        _ => SvgRefusal::UnsupportedElement,
                    });
                }
                if !gradient::valid_parent(name, stack.last().map(String::as_str)) {
                    return Err(SvgRefusal::InvalidStructure);
                }
                let output =
                    sanitized_element(&start, name, usage, require_square, &mut references)?;
                saw_root = true;
                if empty {
                    writer
                        .write_event(Event::Empty(output))
                        .map_err(|_| SvgRefusal::InvalidXml)?;
                    closed_root = stack.is_empty();
                } else {
                    if usage == SvgUse::ChatTile && stack.len() >= 32 {
                        return Err(SvgRefusal::TooDeep);
                    }
                    writer
                        .write_event(Event::Start(output))
                        .map_err(|_| SvgRefusal::InvalidXml)?;
                    stack.push(name.to_owned());
                }
            }
            Event::End(end) => {
                let end_name = end.name();
                let name =
                    std::str::from_utf8(end_name.as_ref()).map_err(|_| SvgRefusal::InvalidXml)?;
                if stack.pop().as_deref() != Some(name) {
                    return Err(SvgRefusal::InvalidStructure);
                }
                writer
                    .write_event(Event::End(end.into_owned()))
                    .map_err(|_| SvgRefusal::InvalidXml)?;
                if stack.is_empty() {
                    closed_root = true;
                }
            }
            Event::Text(text) => {
                if !text
                    .unescape()
                    .map_err(|_| SvgRefusal::InvalidXml)?
                    .chars()
                    .all(char::is_whitespace)
                {
                    return Err(SvgRefusal::UnsupportedMarkup);
                }
            }
            Event::Eof => {
                if !saw_root || !closed_root || !stack.is_empty() {
                    return Err(SvgRefusal::InvalidStructure);
                }
                if !references.valid() {
                    return Err(SvgRefusal::Reference);
                }
                let output = writer.into_inner();
                if output.len() > max_bytes {
                    return Err(SvgRefusal::TooLarge);
                }
                return String::from_utf8(output).map_err(|_| SvgRefusal::InvalidXml);
            }
            Event::CData(_) | Event::Comment(_) | Event::DocType(_) | Event::PI(_) => {
                return Err(SvgRefusal::UnsupportedMarkup);
            }
        }
        buffer.clear();
    }
}

fn sanitized_element(
    element: &BytesStart<'_>,
    name: &str,
    usage: SvgUse,
    require_square: bool,
    references: &mut gradient::References,
) -> Result<BytesStart<'static>, SvgRefusal> {
    let mut output = BytesStart::new(name.to_string());
    if usage == SvgUse::ChatTile && gradient::definition(name) {
        return Err(SvgRefusal::UnsupportedElement);
    }
    let mut has_view_box = false;
    let mut has_namespace = false;
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| SvgRefusal::InvalidXml)?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| SvgRefusal::InvalidXml)?
            .to_string();
        let mut value = attribute
            .unescape_value()
            .map_err(|_| SvgRefusal::InvalidXml)?
            .into_owned();
        if usage == SvgUse::ChatTile
            && matches!(key.as_str(), "d" | "points" | "viewBox" | "transform")
        {
            value = value.replace(['\t', '\r', '\n'], " ");
        }
        if usage == SvgUse::ChatTile {
            check_tile_attribute(&key, &value)?;
        }
        let valid = match (usage, key.as_str()) {
            (SvgUse::ChatTile, "transform") => gradient::transform(&value),
            (SvgUse::ChatTile, "stroke-dasharray") => valid_dash_array(&value),
            _ if !allowed_attribute(name, &key) => return Err(SvgRefusal::UnsupportedAttribute),
            _ if gradient::definition(name) => gradient::attribute_value(&key, &value),
            _ => allowed_attribute_value(&key, &value),
        };
        if !valid {
            return Err(if key == "viewBox" {
                SvgRefusal::InvalidViewBox
            } else {
                SvgRefusal::InvalidAttribute
            });
        }
        if usage == SvgUse::ChatTile {
            if require_square && key == "viewBox" {
                let mut dimensions = value.split_whitespace().skip(2).map(str::parse::<f64>);
                if dimensions.next() != dimensions.next() {
                    return Err(SvgRefusal::NonSquare);
                }
            }
            if name == "svg" && matches!(key.as_str(), "width" | "height") {
                continue;
            }
            has_view_box |= key == "viewBox";
            has_namespace |= key == "xmlns";
        }
        references
            .observe(&key, &value)
            .ok_or(SvgRefusal::InvalidAttribute)?;
        output.push_attribute((key.as_str(), value.as_str()));
    }
    if usage == SvgUse::ChatTile && name == "svg" {
        if !has_view_box {
            return Err(SvgRefusal::MissingViewBox);
        }
        if !has_namespace {
            output.push_attribute(("xmlns", "http://www.w3.org/2000/svg"));
        }
    }
    Ok(output)
}

fn check_tile_attribute(key: &str, value: &str) -> Result<(), SvgRefusal> {
    if key == "style" {
        return Err(SvgRefusal::Style);
    }
    if key.starts_with("on") {
        return Err(SvgRefusal::EventHandler);
    }
    if key != "xmlns" {
        let lower = value.trim().to_ascii_lowercase();
        let href = key.rsplit(':').next() == Some("href");
        if let Some((_, target)) = lower.split_once("url(") {
            return Err(
                if target.trim_start_matches([' ', '\'', '"']).starts_with('#') {
                    SvgRefusal::Reference
                } else {
                    SvgRefusal::ExternalReference
                },
            );
        }
        if href && lower.starts_with('#') {
            return Err(SvgRefusal::Reference);
        }
        if href || lower.contains("://") || lower.starts_with("data:") {
            return Err(SvgRefusal::ExternalReference);
        }
    }
    Ok(())
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
