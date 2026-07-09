pub fn is_codeterm_injection(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("# AGENTS.md instructions")
        || t.starts_with("<INSTRUCTIONS>")
        || t.starts_with("<environment_context>")
        || t.starts_with("<permissions instructions>")
        || t.starts_with("You are a Graph Worker")
        || t.starts_with("You are the General Agent")
}
