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
}
