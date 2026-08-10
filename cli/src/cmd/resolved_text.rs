use regex::Regex;

const RESOLVED_TEXT_PREFIX: &str = "CODETERM_RESOLVED_TEXT=";

/// Mask secret-shaped substrings before a resolved payload is echoed back into
/// the caller's transcript. Every echo path goes through this — there is no
/// unredacted resolved-body echo.
pub fn redact_resolved_text(text: &str) -> String {
    let bearer = Regex::new(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{8,}").unwrap();
    let assignment = Regex::new(r"(?i)\b([a-z][a-z0-9_]*(?:token|secret|password|api[_-]?key|credential)[a-z0-9_]*)=([^\s]+)").unwrap();
    let openai_key = Regex::new(r"\bsk-[A-Za-z0-9_-]{8,}\b").unwrap();
    let redacted = bearer.replace_all(text, "$1***REDACTED***");
    let redacted = assignment.replace_all(&redacted, "$1=***REDACTED***");
    openai_key
        .replace_all(&redacted, "***REDACTED***")
        .into_owned()
}

/// The single resolved-body echo line. Any command whose payload can arrive by
/// staging (pad/file/stdin/doc) prints this so the chat card renders what was
/// delivered instead of what argv happened to carry.
pub fn resolved_text_echo(text: &str) -> String {
    format!(
        "{}{}",
        RESOLVED_TEXT_PREFIX,
        serde_json::to_string(&redact_resolved_text(text)).unwrap()
    )
}

#[cfg(test)]
mod tests {
    use super::{redact_resolved_text, resolved_text_echo, RESOLVED_TEXT_PREFIX};

    #[test]
    fn resolved_text_echo_redacts_secrets() {
        let echo = resolved_text_echo("deliver sk-live-super-secret-token and API_TOKEN=hunter2");
        assert!(echo.starts_with("CODETERM_RESOLVED_TEXT=\"deliver "));
        assert!(echo.contains("***REDACTED***"));
        assert!(!echo.contains("sk-live-super-secret-token"));
        assert!(!echo.contains("hunter2"));
        assert_eq!(
            redact_resolved_text("Authorization: Bearer abcdefghijkl"),
            "Authorization: Bearer ***REDACTED***"
        );
    }

    #[test]
    fn resolved_text_echo_is_one_json_encoded_line() {
        let echo = resolved_text_echo("first line\nsecond \"quoted\" line");
        assert_eq!(echo.lines().count(), 1, "echo must stay a single line: {echo}");
        let encoded = echo
            .strip_prefix(RESOLVED_TEXT_PREFIX)
            .expect("echo carries the shared prefix");
        assert_eq!(
            serde_json::from_str::<String>(encoded).expect("payload is JSON"),
            "first line\nsecond \"quoted\" line"
        );
    }
}
