use super::*;
fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}
#[test]
fn atomic_locator_arguments_require_complete_scope_and_refuse_duplicates() {
    let out = parse(
        &args(&[
            "--within",
            "dialog:Settings",
            "--role",
            "textbox",
            "--name",
            "Title",
            "new value",
        ]),
        true,
    )
    .unwrap();
    assert_eq!(out.text.as_deref(), Some("new value"));
    assert_eq!(out.within, "dialog:Settings");
    for words in [
        vec!["--role", "button"],
        vec!["--within"],
        vec![
            "--within", "dialog:A", "--within", "dialog:B", "--role", "button", "--name", "Save",
        ],
    ] {
        assert!(parse(&args(&words), false).is_err());
    }
}
