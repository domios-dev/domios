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
    static BARE_SKIP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^\s*(skip|later|postpone)\s*$").unwrap());
    let safe: Vec<_> = prompt
        .options
        .iter()
        .filter(|o| SKIP.is_match(&o.label))
        .collect();
    // Codex offers "Skip" and "Skip until next version"; the bare skip is the one-time choice.
    let option = match safe.as_slice() {
        [only] => *only,
        _ => {
            let mut bare = safe.iter().filter(|o| BARE_SKIP.is_match(&o.label));
            let choice = bare.next()?;
            if bare.next().is_some() {
                return None;
            }
            *choice
        }
    };
    if option.value.is_empty()
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
