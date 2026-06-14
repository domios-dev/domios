pub(super) fn opencode_short_label(model_id: &str) -> String {
    let m = model_id
        .rsplit_once('/')
        .map(|(_, model)| model)
        .unwrap_or(model_id)
        .to_ascii_lowercase();
    for fam in [
        "opus", "sonnet", "haiku", "codex", "gemini", "gpt", "deepseek", "qwen", "llama",
        "mistral", "grok",
    ] {
        if m.contains(fam) {
            return capitalize_family(fam);
        }
    }
    let trimmed: String = m.split('-').take(2).collect::<Vec<_>>().join("-");
    if trimmed.len() <= 14 {
        trimmed
    } else {
        m.split('-').next().unwrap_or(&m).to_string()
    }
}

fn capitalize_family(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase(),
        None => String::new(),
    }
}
