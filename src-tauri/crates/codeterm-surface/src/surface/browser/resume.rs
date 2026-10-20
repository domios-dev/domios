use codeterm_browser::profile::sanitize_profile_name;

pub(super) const MAX_RESUME_URL: usize = 2048;
const NOT_A_PROFILE: &[&str] = &["", "clean", "attached"];

/// The page worth reopening after a restart: http(s) or file, no credentials, bounded length.
pub(super) fn resumable_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_RESUME_URL {
        return None;
    }
    let parsed = url::Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https" | "file") {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    Some(raw.to_string())
}

/// The named profile a surface label stands for; the clean and attach modes have none.
pub(super) fn resumable_profile(label: &str) -> Option<String> {
    let label = label.trim();
    if NOT_A_PROFILE
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(label))
    {
        return None;
    }
    (sanitize_profile_name(label) == label).then(|| label.to_string())
}

/// The remembered profile to launch under, or `None` when it no longer exists.
pub(super) fn restored_profile(wanted: &str, available: &[String]) -> Option<String> {
    resumable_profile(wanted).filter(|name| available.iter().any(|known| known == name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_resume_keeps_web_and_file_pages() {
        for url in [
            "https://mail.example.com/inbox?x=1#thread/42",
            "http://127.0.0.1:5173/",
            "file:///C:/Users/me/report.html",
        ] {
            assert_eq!(resumable_url(url).as_deref(), Some(url));
        }
    }

    #[test]
    fn browser_resume_drops_internal_credentialed_and_oversized_urls() {
        for url in [
            "",
            "about:blank",
            "data:text/html,hi",
            "javascript:alert(1)",
            "chrome://settings",
            "chrome-error://chromewebdata/",
            "devtools://devtools/bundled/inspector.html",
            "https://user:pass@example.com/",
            "https://user@example.com/",
            "not a url",
        ] {
            assert_eq!(resumable_url(url), None, "{url}");
        }
        let long = format!("https://a.dev/{}", "x".repeat(MAX_RESUME_URL));
        assert_eq!(resumable_url(&long), None);
        let fits = format!("https://a.dev/{}", "x".repeat(MAX_RESUME_URL - 14));
        assert_eq!(
            resumable_url(&fits).map(|url| url.len()),
            Some(MAX_RESUME_URL)
        );
    }

    #[test]
    fn browser_resume_profile_names_only_named_profiles() {
        assert_eq!(resumable_profile("work").as_deref(), Some("work"));
        assert_eq!(resumable_profile("clean"), None);
        assert_eq!(resumable_profile("attached"), None);
        assert_eq!(resumable_profile(""), None);
        assert_eq!(resumable_profile("../escape"), None);
    }

    #[test]
    fn browser_resume_falls_back_to_clean_when_the_profile_is_gone() {
        let available = vec!["personal".to_string(), "work".to_string()];
        assert_eq!(
            restored_profile("work", &available).as_deref(),
            Some("work")
        );
        assert_eq!(restored_profile("deleted", &available), None);
        assert_eq!(restored_profile("clean", &available), None);
    }
}
