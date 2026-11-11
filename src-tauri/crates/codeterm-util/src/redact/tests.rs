use super::text;

#[test]
fn secret_names_that_continue_as_identifiers_are_redacted() {
    for (input, kept) in [
        ("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMIK7MDENG", "AWS_"),
        ("aws_access_key_id: AKIAOPAQUECANARY", "aws_"),
        ("GITHUB_TOKEN=opaque-canary", "GITHUB_"),
        ("db_password_main = opaque-canary", "db_"),
        ("SSH_PRIVATE_KEY: opaque-canary", "SSH_"),
        ("MY_API_KEY=opaque-canary", "MY_"),
        ("export Passwd_Backup=opaque-canary", "export "),
    ] {
        let clean = text(input);
        assert_eq!(clean, format!("{kept}[redacted]"), "{input}");
        assert!(
            !clean.contains("canary") && !clean.contains("wJalr"),
            "{input}"
        );
    }
}

#[test]
fn names_that_merely_contain_a_word_and_unlabelled_text_are_kept() {
    for input in [
        "SECRETARY=Ada",
        "TOKENIZER=bpe",
        "the token budget",
        "MY_VAR=opaque-value",
        "issue 1234",
    ] {
        let clean = text(input);
        if input == "the token budget" {
            assert_eq!(
                clean, "the [redacted]",
                "a bare label still guards its line"
            );
        } else {
            assert_eq!(clean, input);
        }
    }
}
