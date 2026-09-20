pub(crate) use crate::multiplexer::spawn_title::creation_title_override;

pub(crate) fn validate_agent_tab_title(
    explicit: Option<&str>,
    agent_name: Option<&str>,
) -> Result<(), String> {
    let explicit = explicit.map(normalize_agent_title_value);
    let agent_name = agent_name
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    if let Some(title) = explicit.or(agent_name) {
        crate::service::tab::validate_tab_title(&title)?;
    }
    Ok(())
}

const MSYS_ROOT_PREFIXES: &[&str] = &[
    "c:/program files/git/",
    "c:/program files (x86)/git/",
    "c:/msys64/",
    "c:/msys32/",
];

pub(crate) fn normalize_agent_title(title: Option<String>) -> Option<String> {
    title.map(|value| normalize_agent_title_value(&value))
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
    use crate::multiplexer::types::TitleLevel;

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
            creation_title_override(Some("Scout"), Some("Watson")),
            Some(("Scout".to_string(), TitleLevel::User))
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
        assert_eq!(creation_title_override(None, None), None);
    }

    #[test]
    fn agent_title_accepts_one_word_and_rejects_prose() {
        assert!(validate_agent_tab_title(Some("Scout"), None).is_ok());
        let error = validate_agent_tab_title(Some("Warehouse party"), None).unwrap_err();
        assert!(error.contains("one word"));
        assert!(error.contains("description"));
    }
}
