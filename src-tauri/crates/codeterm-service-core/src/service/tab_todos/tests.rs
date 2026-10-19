use super::*;

#[test]
fn acceptance_seed_titles_count_unicode_and_record_truncation() {
    let source = "🦀".repeat(121);
    let (title, truncated) = bounded_title(&source);
    assert_eq!(title.chars().count(), 120);
    assert!(truncated);
    assert!(title.is_char_boundary(title.len()));
}

#[test]
fn recitation_payload_is_bounded_and_contains_no_more_than_three_titles() {
    let recitation = TabTodoRecitation {
        tab_id: "tab".into(),
        generation: 2,
        revision: 7,
        items: (0..MAX_TAB_TODO_ITEMS)
            .map(|index| TabTodoRecitedItem {
                id: format!("item-{index}"),
                title: "🦀".repeat(120),
                state: TabTodoState::Open,
            })
            .collect(),
    };
    let text = recitation_text(&recitation);
    assert!(text.len() <= MAX_TAB_TODO_SUMMARY_BYTES);
    assert!(text.matches("\n-").count() <= MAX_TAB_TODO_SUMMARY_TITLES);
}

#[test]
fn external_url_requires_http_or_https_host() {
    assert!(validate_external_url("https://example.test/task/1").is_ok());
    assert!(validate_external_url("file:///tmp/task").is_err());
}
