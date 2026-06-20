use super::{is_vertical_border, strip_selected_glyph};

pub(super) fn strip_side_band(text: &str) -> String {
    let trimmed = text.trim_end();
    let mut s = trimmed.trim_start();
    if let Some(rest) = s.strip_prefix('┃') {
        s = rest.trim_start();
        for label in ["Ask", "Buil", "Build"] {
            if let Some(rest) = s.strip_prefix(label) {
                if rest.starts_with("  ") {
                    s = rest.trim_start();
                    break;
                }
            }
        }
        return preserve_indent(trimmed, s);
    }
    if s.starts_with('╹') {
        s = s.trim_start_matches(|c| c == '╹' || c == '▀' || c == ' ');
        let s = s.split("  ▀").next().unwrap_or(s).trim();
        return preserve_indent(trimmed, s);
    }
    trimmed.to_string()
}

pub(super) fn strip_side_preview(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let Some(idx) = chars
        .iter()
        .enumerate()
        .skip(12)
        .find(|(_, ch)| is_side_preview_start(**ch))
        .map(|(idx, _)| idx)
    else {
        return text.to_string();
    };
    if chars[..idx]
        .iter()
        .rev()
        .take(4)
        .filter(|ch| ch.is_whitespace())
        .count()
        < 2
    {
        return text.to_string();
    }
    chars[..idx]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string()
}

pub(super) fn looks_like_path(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
}

pub(super) fn looks_like_section_tabs(text: &str) -> bool {
    let lower = text.to_lowercase();
    (text.contains('☐') || text.contains('✔')) && (lower.contains("submit") || text.contains('→'))
}

pub(super) fn shortcut_token(text: &str) -> Option<&str> {
    shortcut_token_and_rest(text).map(|(token, _)| token)
}

pub(super) fn shortcut_token_and_rest(text: &str) -> Option<(&str, &str)> {
    let s = strip_selected_glyph(text.trim_start());
    let rest = s.strip_prefix('[')?;
    let end = rest.find(']')?;
    let token = &rest[..end];
    let after = rest.get(end + 1..)?;
    if token.len() == 1
        && token.chars().all(|c| c.is_ascii_alphanumeric())
        && after.starts_with(char::is_whitespace)
    {
        Some((token, after))
    } else {
        None
    }
}

pub(super) fn starts_with_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.starts_with(needle))
}

fn is_side_preview_start(ch: char) -> bool {
    is_vertical_border(ch) || matches!(ch, '┌' | '└' | '╭' | '╰')
}

fn preserve_indent(original: &str, text: &str) -> String {
    let indent = original.chars().take_while(|c| c.is_whitespace()).count();
    format!("{}{}", " ".repeat(indent), text)
}
