pub(crate) const NEEDS_NONE: &str = "none";
pub(crate) const IDLE: &str = "idle";

/// Whether the page needs a human before an agent can continue; ids match the frontend capsule.
pub(crate) fn needs_human(url: &str, title: &str, dialog_open: bool) -> &'static str {
    let url = url.to_ascii_lowercase();
    let title = title.to_ascii_lowercase();
    let url_has = |needles: &[&str]| needles.iter().any(|needle| url.contains(needle));
    let title_has = |needles: &[&str]| needles.iter().any(|needle| title.contains(needle));
    if dialog_open {
        "dialog"
    } else if url_has(&["/sorry/", "captcha", "challenge"])
        || title_has(&["unusual traffic", "are you a robot"])
    {
        "captcha"
    } else if title_has(&["before you continue", "cookie"]) {
        "consent"
    } else if url_has(&["/login", "/signin", "accounts.google.com"]) {
        "login"
    } else {
        NEEDS_NONE
    }
}

/// Verbs that only read core state leave the capsule's activity alone.
pub(crate) fn is_quiet_verb(verb: &str) -> bool {
    matches!(verb, "status" | "profiles" | "inbox" | "endpoint" | "help")
}

pub(crate) fn activity_label(verb: &str, args: &[String]) -> String {
    let first = args.first().map(String::as_str).unwrap_or("");
    let target = if first.starts_with('@') {
        first.to_string()
    } else if args.len() > 1 {
        format!("'{}'", args[1])
    } else {
        first.to_string()
    };
    match verb {
        "open" => "opening the browser".into(),
        "goto" => format!("navigating to {first}"),
        "snapshot" => "reading the page".into(),
        "click" => format!("clicking {target}"),
        "type" => format!("typing into {target}"),
        "press" => format!("pressing {first}"),
        "scroll" => "scrolling".into(),
        "back" => "going back".into(),
        "forward" => "going forward".into(),
        "reload" => "reloading".into(),
        "text" => "reading text".into(),
        "wait" => format!(
            "waiting for {}",
            args.get(1..).unwrap_or_default().join(" ")
        ),
        "screenshot" => "taking a screenshot".into(),
        "annotate" => format!("annotating {target}"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn browser_needs_human_names_the_blocking_page_state() {
        assert_eq!(needs_human("https://a.dev", "A", true), "dialog");
        assert_eq!(
            needs_human("https://www.google.com/sorry/index", "", false),
            "captcha"
        );
        assert_eq!(
            needs_human("https://x.dev", "Are you a robot?", false),
            "captcha"
        );
        assert_eq!(
            needs_human("https://x.dev", "Before you continue to Google", false),
            "consent"
        );
        assert_eq!(
            needs_human("https://accounts.google.com/v3/signin", "Sign in", false),
            "login"
        );
        assert_eq!(needs_human("https://x.dev/app", "App", false), NEEDS_NONE);
    }

    #[test]
    fn browser_activity_labels_name_the_target() {
        assert_eq!(activity_label("click", &args(&["@e3"])), "clicking @e3");
        assert_eq!(
            activity_label("click", &args(&["button", "Sign in"])),
            "clicking 'Sign in'"
        );
        assert_eq!(
            activity_label("wait", &args(&["--text", "Done", "now"])),
            "waiting for Done now"
        );
        assert_eq!(activity_label("wait", &[]), "waiting for ");
        assert_eq!(
            activity_label("goto", &args(&["a.dev"])),
            "navigating to a.dev"
        );
        assert!(is_quiet_verb("inbox"));
        assert!(!is_quiet_verb("click"));
    }
}
