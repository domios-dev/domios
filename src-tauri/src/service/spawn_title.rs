use crate::multiplexer::types::TitleLevel;

const MSYS_ROOT_PREFIXES: &[&str] = &[
    "c:/program files/git/",
    "c:/program files (x86)/git/",
    "c:/msys64/",
    "c:/msys32/",
];

pub(crate) fn normalize_agent_title(title: Option<String>) -> Option<String> {
    title.map(|value| normalize_agent_title_value(&value))
}

/// The single decision seam for whether a freshly spawned pane's title should
/// override the `create_tab`-generated name. Returns `None` when neither a
/// real explicit title nor a real agent name was supplied — callers MUST then
/// leave the generated name alone; the raw provider id / agent_cmd is never
/// an acceptable title on any spawn path.
pub(crate) fn creation_title_override(
    explicit: Option<&str>,
    agent_name: Option<&str>,
) -> Option<(String, TitleLevel)> {
    explicit
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or_else(|| agent_name.map(str::trim).filter(|n| !n.is_empty()))
        .map(|t| (t.to_string(), TitleLevel::User))
}

fn normalize_agent_title_value(title: &str) -> String {
    let normalized = title.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    for prefix in MSYS_ROOT_PREFIXES {
        if !lower.starts_with(prefix) {
            continue;
        }
        let stripped = normalized[prefix.len()..].trim_start_matches('/');
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }
    title.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_git_bash_root_from_path_converted_title() {
        assert_eq!(
            normalize_agent_title_value("C:/Program Files/Git/web 404 investigator"),
            "web 404 investigator"
        );
    }

    #[test]
    fn strips_backslash_git_bash_root_from_path_converted_title() {
        assert_eq!(
            normalize_agent_title_value("C:\\Program Files\\Git\\web 404 investigator"),
            "web 404 investigator"
        );
    }

    #[test]
    fn leaves_normal_titles_unchanged() {
        assert_eq!(
            normalize_agent_title_value("web 404 investigator"),
            "web 404 investigator"
        );
        assert_eq!(
            normalize_agent_title_value("C:/Projects/Git/web"),
            "C:/Projects/Git/web"
        );
    }

    #[test]
    fn no_title_or_agent_name_yields_no_override() {
        assert_eq!(creation_title_override(None, None), None);
        assert_eq!(creation_title_override(Some("   "), Some("")), None);
    }

    #[test]
    fn explicit_title_wins_at_user_level() {
        assert_eq!(
            creation_title_override(Some("my task"), Some("Watson")),
            Some(("my task".to_string(), TitleLevel::User))
        );
    }

    #[test]
    fn agent_name_used_when_no_explicit_title() {
        assert_eq!(
            creation_title_override(None, Some("Watson")),
            Some(("Watson".to_string(), TitleLevel::User))
        );
    }

    #[test]
    fn provider_id_is_never_returned_as_a_fallback_title() {
        // The caller must NOT pass agent_cmd as either argument — this test
        // documents the contract: with nothing real supplied, the answer is
        // always None, never Some(provider_id).
        assert_eq!(creation_title_override(None, None), None);
    }
}
