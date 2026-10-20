pub(super) const FIRST_WEEK_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_CAPABILITIES: usize = 16;
const MAX_LINE_BYTES: usize = 240;
const MAX_FRAGMENT_BYTES: usize = 4096;
const RULE: &str = "These installed Domios capabilities exist. Mention one only when it directly helps the user's request, in your own words, at most once per topic. Never advertise unprompted.";

#[derive(Clone)]
pub(crate) struct Capability {
    pub id: String,
    pub purpose: String,
}

pub(super) fn build(
    capabilities: &[Capability],
    now: u64,
    first_seen: u64,
    enabled: bool,
) -> Option<String> {
    if !enabled || now.saturating_sub(first_seen) >= FIRST_WEEK_SECONDS {
        return None;
    }
    let mut fragment = format!("\n\n## Relevant capabilities\n\n{RULE}\n");
    for capability in capabilities.iter().take(MAX_CAPABILITIES) {
        let line = single_line(&format!("{}: {}", capability.id, capability.purpose));
        if fragment.len() + line.len() + 3 > MAX_FRAGMENT_BYTES {
            break;
        }
        fragment.push_str(&format!("- {line}\n"));
    }
    Some(fragment)
}

fn single_line(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if result.len() + ch.len_utf8() > MAX_LINE_BYTES {
            break;
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests;
