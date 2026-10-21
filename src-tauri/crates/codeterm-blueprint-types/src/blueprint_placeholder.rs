use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub struct Placeholder<'a> {
    pub span: Range<usize>,
    pub key: Option<&'a str>,
    pub closed: bool,
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key.as_bytes()[0] != b'.'
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub fn tokens(text: &str) -> Vec<Placeholder<'_>> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(start) = text[offset..].find("{{") {
        let start = offset + start;
        let Some(end) = text[start + 2..].find("}}") else {
            result.push(Placeholder {
                span: start..text.len(),
                key: None,
                closed: false,
            });
            break;
        };
        let end = start + 2 + end;
        let key = text[start + 2..end].trim();
        result.push(Placeholder {
            span: start..end + 2,
            key: valid_key(key).then_some(key),
            closed: true,
        });
        offset = end + 2;
    }
    result
}

#[cfg(test)]
mod tests;
