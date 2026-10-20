use super::*;

pub(super) fn has_selected_glyph(text: &str) -> bool {
    starts_with_any(text, &["❯", "›", ">", "→", "➜", "▸", "▶", "●"])
}

pub(super) fn contains_checkbox(text: &str) -> bool {
    ["[ ]", "[x]", "[X]", "( )", "(x)", "(X)", "☐", "☑", "☒"]
        .iter()
        .any(|needle| text.contains(needle))
}

pub(super) fn has_checked_box(text: &str) -> bool {
    ["[x]", "[X]", "(x)", "(X)", "☑", "☒"]
        .iter()
        .any(|needle| text.contains(needle))
}

pub(super) fn contains_select_all_hint(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("select all") || lower.contains("space select") || lower.contains("tab select")
}

pub(super) fn is_footer_hint(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("interrupt") {
        return false;
    }
    lower.contains("enter to select")
        || lower.contains("space select")
        || lower.contains("tab toggle")
        || lower.contains("tab select")
        || lower.contains("esc dismiss")
        || lower.contains("esc to")
        || lower.contains("←/→")
        || lower.contains("left/right")
}

#[cfg(test)]
mod tests {
    use super::is_footer_hint;

    #[test]
    fn running_control_hint_is_not_a_picker_footer() {
        assert!(!is_footer_hint("esc to interrupt"));
        assert!(is_footer_hint("Esc to cancel"));
    }
}

pub(super) fn is_box_border(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && (trimmed.chars().filter(|c| is_border_char(*c)).count() * 2 >= trimmed.chars().count()
            || trimmed.chars().filter(|c| is_border_char(*c)).count() >= 6)
}

pub(super) fn is_vertical_border(c: char) -> bool {
    matches!(c, '│' | '┃' | '║')
}
