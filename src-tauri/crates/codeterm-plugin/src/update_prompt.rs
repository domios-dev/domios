use codeterm_types::pty::PendingPrompt;
use regex::Regex;
use std::sync::LazyLock;

pub fn safe_choice(prompt: &PendingPrompt) -> Option<String> {
    static UPDATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bupdate\b").unwrap());
    static SKIP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(skip|later|postpone)\b").unwrap());
    if prompt.options.len() < 2 || !prompt.options.iter().any(|o| UPDATE.is_match(&o.label)) {
        return None;
    }
    let mut safe = prompt.options.iter().filter(|o| SKIP.is_match(&o.label));
    let option = safe.next()?;
    if safe.next().is_some()
        || option.value.is_empty()
        || UPDATE.is_match(&option.label)
        || prompt
            .options
            .iter()
            .filter(|o| o.value == option.value)
            .count()
            != 1
    {
        return None;
    }
    Some(option.value.clone())
}
