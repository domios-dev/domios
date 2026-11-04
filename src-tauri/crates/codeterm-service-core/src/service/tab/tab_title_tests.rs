use super::{validate_tab_title, TAB_TITLE_ERROR};

#[test]
fn accepts_one_word_and_rejects_prose() {
    assert!(validate_tab_title("Dalton").is_ok());
    assert_eq!(
        validate_tab_title("Warehouse party").unwrap_err(),
        TAB_TITLE_ERROR
    );
    assert_eq!(
        validate_tab_title("Review\u{00a0}queue").unwrap_err(),
        TAB_TITLE_ERROR
    );
}
