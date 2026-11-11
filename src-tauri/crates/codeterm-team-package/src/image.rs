use crate::diagnostics::DiagnosticCode;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Svg,
    Png,
}

impl IconKind {
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Png => "image/png",
        }
    }
}

type Refusal = (DiagnosticCode, String);

fn not_image(path: &str, why: &str) -> Refusal {
    (
        DiagnosticCode::IconNotImage,
        format!("icon `{path}` is not a usable image: {why}"),
    )
}

/// Validates an icon by its bytes; the extension only names which format must match.
pub fn check_icon(path: &str, bytes: &[u8]) -> Result<IconKind, (DiagnosticCode, String)> {
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "png" => {
            let valid =
                bytes.len() > 16 && bytes[..8] == PNG_SIGNATURE && &bytes[12..16] == b"IHDR";
            if valid {
                Ok(IconKind::Png)
            } else {
                Err(not_image(path, "the bytes are not a PNG image"))
            }
        }
        "svg" => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| not_image(path, "an SVG icon must be UTF-8 text"))?;
            check_svg(path, text).map(|()| IconKind::Svg)
        }
        _ => Err(not_image(path, "icons are .svg or .png files")),
    }
}

fn external(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.starts_with('#')
}

fn css_references(path: &str, text: &str) -> Result<(), Refusal> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("@import") || lower.contains("javascript:") {
        return Err((
            DiagnosticCode::SvgExternalReference,
            format!("icon `{path}` imports or executes external content"),
        ));
    }
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("url(") {
        rest = &rest[start + 4..];
        let target = rest.split(')').next().unwrap_or("");
        let target = target.trim().trim_matches(['"', '\'']);
        if external(target) {
            return Err((
                DiagnosticCode::SvgExternalReference,
                format!("icon `{path}` references `{target}`; only in-document `#` references are allowed"),
            ));
        }
    }
    Ok(())
}

struct Tag<'a> {
    name: String,
    attributes: Vec<(String, &'a str)>,
}

/// Splits the inside of `<...>` into a lowercase element name and its attributes.
fn parse_tag(body: &str) -> Option<Tag<'_>> {
    let body = body.trim_end_matches('/').trim();
    let name_end = body
        .find(|character: char| character.is_whitespace())
        .unwrap_or(body.len());
    let name = body[..name_end].to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    let mut attributes = Vec::new();
    let mut rest = body[name_end..].trim_start();
    while !rest.is_empty() {
        let equals = rest.find('=');
        let space = rest.find(char::is_whitespace);
        match (equals, space) {
            (Some(equals), space) if space.is_none_or(|space| equals < space) => {
                let key = rest[..equals].trim().to_ascii_lowercase();
                let after = rest[equals + 1..].trim_start();
                let quote = after.chars().next()?;
                if quote != '"' && quote != '\'' {
                    return None;
                }
                let close = after[1..].find(quote)?;
                attributes.push((key, &after[1..1 + close]));
                rest = after[close + 2..].trim_start();
            }
            (_, Some(space)) => {
                attributes.push((rest[..space].to_ascii_lowercase(), ""));
                rest = rest[space..].trim_start();
            }
            (_, None) => {
                attributes.push((rest.to_ascii_lowercase(), ""));
                rest = "";
            }
        }
    }
    Some(Tag { name, attributes })
}

/// A conservative SVG check: one `svg` root, no script, handler, DTD or external reference.
fn check_svg(path: &str, text: &str) -> Result<(), Refusal> {
    let mut rest = text;
    let mut root: Option<String> = None;
    while let Some(open) = rest.find('<') {
        css_references(path, &rest[..open])?;
        rest = &rest[open + 1..];
        if let Some(comment) = rest.strip_prefix("!--") {
            let end = comment
                .find("-->")
                .ok_or_else(|| not_image(path, "an XML comment is not closed"))?;
            rest = &comment[end + 3..];
            continue;
        }
        if let Some(cdata) = rest.strip_prefix("![CDATA[") {
            let end = cdata
                .find("]]>")
                .ok_or_else(|| not_image(path, "a CDATA section is not closed"))?;
            css_references(path, &cdata[..end])?;
            rest = &cdata[end + 3..];
            continue;
        }
        if rest.starts_with('!') {
            return Err((
                DiagnosticCode::SvgExternalReference,
                format!("icon `{path}` declares a DTD, which can pull external entities"),
            ));
        }
        if let Some(instruction) = rest.strip_prefix('?') {
            let end = instruction
                .find("?>")
                .ok_or_else(|| not_image(path, "a processing instruction is not closed"))?;
            rest = &instruction[end + 2..];
            continue;
        }
        let end = rest
            .find('>')
            .ok_or_else(|| not_image(path, "a tag is not closed"))?;
        let body = &rest[..end];
        rest = &rest[end + 1..];
        if body.starts_with('/') {
            continue;
        }
        let tag = parse_tag(body).ok_or_else(|| not_image(path, "a tag cannot be read"))?;
        if root.is_none() {
            if tag.name != "svg" {
                return Err(not_image(path, "the root element is not <svg>"));
            }
            root = Some(tag.name.clone());
        }
        let local = tag.name.rsplit(':').next().unwrap_or(&tag.name);
        if local == "script" || local == "foreignobject" {
            return Err((
                DiagnosticCode::SvgScript,
                format!("icon `{path}` contains <{}>", tag.name),
            ));
        }
        for (key, value) in &tag.attributes {
            if key.starts_with("on") {
                return Err((
                    DiagnosticCode::SvgScript,
                    format!("icon `{path}` has the event handler `{key}`"),
                ));
            }
            if (key == "href" || key.ends_with(":href") || key == "src") && external(value) {
                return Err((
                    DiagnosticCode::SvgExternalReference,
                    format!("icon `{path}` links `{value}`; only in-document `#` references are allowed"),
                ));
            }
            css_references(path, value)?;
        }
    }
    css_references(path, rest)?;
    if root.is_none() {
        return Err(not_image(path, "no <svg> element was found"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
