const MAX_BYTES: usize = 8192;

pub use codeterm_util::redact::text;

pub fn screen(input: &str) -> String {
    let redacted = text(&super::screen_tail(input));
    let mut start = redacted.len().saturating_sub(MAX_BYTES);
    while !redacted.is_char_boundary(start) {
        start += 1;
    }
    redacted[start..].to_string()
}

pub(crate) fn complete_screen(input: &str) -> bool {
    input.trim_end().lines().count() <= super::SCREEN_LINES
        && screen(input) == super::screen_tail(input)
}

pub use codeterm_supporter::privacy::value;
