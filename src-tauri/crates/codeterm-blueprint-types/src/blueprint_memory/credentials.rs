pub fn contains_credentials(text: &str) -> bool {
    static PATTERNS: std::sync::OnceLock<Option<regex::RegexSet>> = std::sync::OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            regex::RegexSet::new([
                r"\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{20,}\b",
                r"\bAKIA[0-9A-Z]{16}\b",
                r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
                r"\bgithub_pat_[A-Za-z0-9_]{22,}\b",
                r#"(?i)\b(?:(?:access|refresh|auth)[_-]?token|token|password|(?:client[_-])?secret|(?:api|private|signing|access)[_-]?key|authorization)\s*[:=]\s*["']?[^\s"'{}]+"#,
                r"(?i)\b[a-z][a-z0-9+.-]*://[^\s/?#@]+(?::[^\s/?#@]*)?@",
            ])
            .ok()
        })
        .as_ref()
        .is_none_or(|patterns| patterns.is_match(text)) || contains_auth_token(text)
}

fn contains_auth_token(text: &str) -> bool {
    static AUTH: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let Some(pattern) = AUTH.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(?:bearer|basic)\s+([A-Za-z0-9+/_=.-]{8,})").ok()
    }) else {
        return true;
    };
    pattern.captures_iter(text).any(|captures| {
        let Some(token) = captures.get(1).map(|capture| capture.as_str()) else {
            return false;
        };
        token
            .bytes()
            .any(|byte| byte.is_ascii_digit() || b"+/_=.-".contains(&byte))
            || (token.bytes().filter(u8::is_ascii_uppercase).count() >= 2
                && token.bytes().filter(u8::is_ascii_lowercase).count() >= 2)
            || token.len() >= 20
    })
}

#[cfg(test)]
mod tests;
