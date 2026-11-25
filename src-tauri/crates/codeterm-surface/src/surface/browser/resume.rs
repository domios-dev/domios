use codeterm_browser::profile::{sanitize_profile_name, ProfileLayout};

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

/// Validates that the persisted profile path actually exists on disk before returning its canonical
/// directory name; prevents folding a deleted spelling onto a differently cased directory on
/// case-sensitive volumes (e.g. case-sensitive APFS on macOS).
pub(super) fn restored_profile_in_layout(layout: &ProfileLayout, wanted: &str) -> Option<String> {
    let name = resumable_profile(wanted)?;
    layout.resolve_named(&name).ok()
}

/// Pure profile-restore matcher: requires `exact_path_exists` (proving the persisted path resolves
/// to a real directory on this volume) before preferring exact spelling and optional case-folding.
#[cfg(test)]
pub(super) fn restored_profile_matching(
    wanted: &str,
    available: &[String],
    exact_path_exists: bool,
    case_insensitive: bool,
) -> Option<String> {
    if !exact_path_exists {
        return None;
    }
    let name = resumable_profile(wanted)?;
    codeterm_browser::profile::match_profile_name(&name, available, case_insensitive)
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
        let root = tempfile::tempdir().unwrap();
        let layout = ProfileLayout::new(root.path().to_path_buf());
        layout.create_named("personal").unwrap();
        layout.create_named("work").unwrap();
        assert_eq!(
            restored_profile_in_layout(&layout, "work").as_deref(),
            Some("work")
        );
        assert_eq!(restored_profile_in_layout(&layout, "deleted"), None);
        assert_eq!(restored_profile_in_layout(&layout, "clean"), None);
    }

    #[test]
    fn browser_resume_resolves_profile_casing_only_when_persisted_path_exists() {
        let available = vec!["andrey".to_string(), "Work".to_string()];
        // Case-insensitive volume: persisted path resolves on disk to the same directory.
        assert_eq!(
            restored_profile_matching("Andrey", &available, true, true).as_deref(),
            Some("andrey")
        );
        assert_eq!(
            restored_profile_matching("work", &available, true, true).as_deref(),
            Some("Work")
        );
        // Case-sensitive volume (e.g. case-sensitive APFS on macOS): deleted "Work" does not exist
        // on disk even though "work" is in `available`, so restore must fall back to clean (None).
        let lower_only = vec!["work".to_string()];
        assert_eq!(
            restored_profile_matching("Work", &lower_only, false, true),
            None
        );
        assert_eq!(
            restored_profile_matching("Andrey", &available, true, false),
            None
        );
        assert_eq!(
            restored_profile_matching("Work", &available, true, false).as_deref(),
            Some("Work")
        );
        // When both spellings exist on a case-sensitive volume, exact match wins.
        let both = vec!["Work".to_string(), "work".to_string()];
        assert_eq!(
            restored_profile_matching("work", &both, true, true).as_deref(),
            Some("work")
        );
        assert_eq!(
            restored_profile_matching("Work", &both, true, true).as_deref(),
            Some("Work")
        );
    }
}
