use std::collections::HashMap;

mod html;
mod scanner;
pub use scanner::scan_artifacts;

const KINDS: &[&str] = &[
    "html", "page", "image", "document", "guide", "plugin", "connect", "setting", "agent",
    "job", "checklist", "choice", "tasks",
];
const HEADER_LIMIT: usize = 16_384;

#[derive(Debug, PartialEq, Eq)]
pub struct FileCard {
    pub start: usize,
    pub end: usize,
    pub kind: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ArtifactScan {
    pub cards: Vec<FileCard>,
    pub attachment_paths: Vec<String>,
    pub overflow: bool,
}

pub fn absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    !path.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
        && (path.starts_with('/')
            || path.starts_with("\\\\")
            || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\')))
}

pub fn escape_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
        .replace('<', "&lt;").replace('>', "&gt;")
}

pub fn serialize_artifact(kind: &str, paths: &[String]) -> Option<String> {
    let limit = if kind == "image" { 32 } else { 1 };
    if !matches!(kind, "page" | "document" | "image") || paths.is_empty()
        || paths.len() > limit
        || !paths.iter().all(|path| absolute(path)) { return None; }
    let (attribute, value) = if kind == "image" && paths.len() > 1 {
        ("sources", serde_json::to_string(paths).ok()?)
    } else { ("src", paths[0].clone()) };
    Some(format!("<ds-{kind} {attribute}=\"{}\" />", escape_attribute(&value)))
}

fn space(byte: u8) -> bool { matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c') }

fn decode_entities(value: &str) -> String {
    static ENTITY: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||
        regex::Regex::new(r"&(?:amp|lt|gt|quot|apos|#\d+|#x[\da-fA-F]+);").unwrap());
    ENTITY.replace_all(value, |caps: &regex::Captures<'_>| {
        match &caps[0] {
            "&amp;" => "&".into(), "&lt;" => "<".into(), "&gt;" => ">".into(),
            "&quot;" => "\"".into(), "&apos;" => "'".into(),
            entity => {
                let (digits, radix) = if entity.starts_with("&#x") {
                    (&entity[3..entity.len()-1], 16)
                } else { (&entity[2..entity.len()-1], 10) };
                u32::from_str_radix(digits, radix).ok().filter(|code| *code > 0)
                    .and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_else(|| entity.into())
            }
        }
    }).into_owned()
}

fn attributes(header: &str, kind: &str) -> Option<HashMap<String, String>> {
    let bytes = header.as_bytes();
    let mut at = 4 + kind.len();
    let mut values = HashMap::new();
    while at < bytes.len() {
        let before = at;
        while at < bytes.len() && space(bytes[at]) { at += 1; }
        if matches!(&header[at..], ">" | "/>") { return Some(values); }
        if before == at { return None; }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_lowercase() { at += 1; }
        if at == start || bytes.get(at) != Some(&b'=') { return None; }
        let name = &header[start..at];
        at += 1;
        let quote = *bytes.get(at)?;
        if !matches!(quote, b'\'' | b'"') { return None; }
        at += 1;
        let start = at;
        while at < bytes.len() && bytes[at] != quote { at += 1; }
        if at == bytes.len() || values.contains_key(name) { return None; }
        values.insert(name.to_string(), decode_entities(&header[start..at]));
        at += 1;
    }
    None
}

fn file_paths(header: &str, kind: &str) -> Option<Vec<String>> {
    if !header.ends_with("/>") || header.encode_utf16().count() > HEADER_LIMIT { return None; }
    let values = attributes(header, kind)?;
    if values.keys().any(|name| name != "src" && !(kind == "image" && name == "sources")) { return None; }
    let paths = match (values.get("src"), values.get("sources")) {
        (Some(src), None) => vec![src.clone()],
        (None, Some(sources)) if kind == "image" => serde_json::from_str::<Vec<String>>(sources).ok()?,
        _ => return None,
    };
    let limit = if kind == "image" { 32 } else { 1 };
    (!paths.is_empty() && paths.len() <= limit
        && paths.iter().all(|path| absolute(path))).then_some(paths)
}

#[cfg(test)]
mod tests;
