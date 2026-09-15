//! Telegram HTML for thread posts, pointers and notices.

use super::identity::{truncate_chars, TOPIC_NAME_MAX_CHARS};

pub const TOPICS_DISABLED_NOTICE: &str =
    "Enable topics for the bot in BotFather to talk to agents in threads.";
const CLOSED_TOPIC_PREFIX: &str = "[closed] ";
const CLOSING_NOTE: &str =
    "This tab was closed. Messages sent here now reach the General Agent.";
const POINTER_PREVIEW_CHARS: usize = 80;

pub(super) fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(super) fn with_header(header: &str, body_html: &str) -> String {
    let header = format!("<b>{}</b>", escape_html(header));
    if body_html.trim().is_empty() {
        header
    } else {
        format!("{header}\n\n{body_html}")
    }
}

pub(super) fn preview(markdown: &str) -> String {
    let first = markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if first.chars().count() > POINTER_PREVIEW_CHARS {
        format!("{}…", truncate_chars(first, POINTER_PREVIEW_CHARS))
    } else {
        first.to_string()
    }
}

pub(super) fn pointer(header: &str, link: Option<&str>, markdown: &str) -> String {
    let mut lines = vec![format!("<b>{}</b>", escape_html(header))];
    let preview = preview(markdown);
    if !preview.is_empty() {
        lines.push(escape_html(&preview));
    }
    match link {
        Some(link) => lines.push(format!("<a href=\"{}\">Open thread</a>", escape_html(link))),
        None => lines.push("Posted in this tab's topic.".to_string()),
    }
    lines.join("\n")
}

pub(super) fn intro(header: &str, title: &str) -> String {
    with_header(header, &escape_html(&format!("you can talk to {title} here")))
}

pub(super) fn closing_note() -> String {
    escape_html(CLOSING_NOTE)
}

pub(super) fn notice() -> String {
    escape_html(TOPICS_DISABLED_NOTICE)
}

pub(super) fn closed_topic_name(name: &str) -> String {
    if name.starts_with(CLOSED_TOPIC_PREFIX) {
        return truncate_chars(name, TOPIC_NAME_MAX_CHARS);
    }
    truncate_chars(&format!("{CLOSED_TOPIC_PREFIX}{name}"), TOPIC_NAME_MAX_CHARS)
}
