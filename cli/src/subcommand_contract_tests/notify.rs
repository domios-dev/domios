//! `notify send` takes its body from the calling tab's pad and from nowhere
//! else. Every other source is a clap-level conflict, so the refusal happens at
//! parse time and cannot be reached past by a runtime branch.

use clap::error::ErrorKind;
use clap::Parser;

use crate::args::misc::{NotifyAction, PasteAction};
use crate::{Cli, Commands};

fn parse_error(argv: &[&str]) -> clap::Error {
    match Cli::try_parse_from(argv) {
        Ok(_) => panic!("this form must be refused at parse time"),
        Err(error) => error,
    }
}

#[test]
fn notify_pad_body_refuses_an_inline_positional_body() {
    let error = parse_error(&["codeterm", "notify", "send", "Needs input", "the body"]);
    assert_eq!(
        error.kind(),
        ErrorKind::ArgumentConflict,
        "an inline body must lose to a declared conflict, not to a runtime check"
    );
}

#[test]
fn notify_pad_body_refuses_stdin_and_file_sources() {
    for argv in [
        vec!["codeterm", "notify", "send", "Needs input", "--from-stdin"],
        vec![
            "codeterm",
            "notify",
            "send",
            "Needs input",
            "--from-file",
            "body.md",
        ],
    ] {
        assert_eq!(
            parse_error(&argv).kind(),
            ErrorKind::ArgumentConflict,
            "{argv:?} must be refused by the same conflict mechanism"
        );
    }
}

/// The pad path is the one source that survives parsing.
#[test]
fn notify_pad_body_accepts_a_pad_relative_path() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "notify",
        "send",
        "Needs input",
        "--from-pad",
        "message.md",
        "--level",
        "action_needed",
    ])
    .expect("--from-pad is the accepted body source");
    assert!(matches!(
        parsed.command,
        Commands::Notify {
            action: NotifyAction::Send {
                from_pad: Some(ref path),
                body: None,
                from_file: None,
                from_stdin: false,
                ..
            }
        } if path == "message.md"
    ));
}

/// Negative control: `paste put`'s shape was copied, not edited. Its inline
/// body, `--from-file` and `--from-stdin` all still parse, and only its own
/// declared conflict with `--from-pad` fires.
#[test]
fn notify_pad_body_leaves_paste_put_unchanged() {
    let inline = Cli::try_parse_from(["codeterm", "paste", "put", "raw body"])
        .expect("paste put still takes an inline body");
    assert!(matches!(
        inline.command,
        Commands::Paste {
            action: PasteAction::Put { body: Some(ref b), .. }
        } if b == "raw body"
    ));

    for argv in [
        vec!["codeterm", "paste", "put", "--from-stdin"],
        vec!["codeterm", "paste", "put", "--from-file", "body.md"],
        vec!["codeterm", "paste", "put", "--from-pad", "message.md"],
    ] {
        assert!(
            Cli::try_parse_from(argv.clone()).is_ok(),
            "{argv:?} must still parse for paste put"
        );
    }

    let conflict = parse_error(&["codeterm", "paste", "put", "raw body", "--from-pad", "m.md"]);
    assert_eq!(conflict.kind(), ErrorKind::ArgumentConflict);
}

/// A repeated `--choice` is the whole choice surface: labels only, in the order
/// they were named, and no sibling flag that would take typed text.
#[test]
fn notify_answer_choice_flag_repeats_and_takes_labels_only() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "notify",
        "send",
        "Which branch?",
        "--choice",
        "Rebase",
        "--choice",
        "Merge",
    ])
    .expect("--choice repeats");
    assert!(matches!(
        parsed.command,
        Commands::Notify {
            action: NotifyAction::Send { ref choice, .. }
        } if choice == &["Rebase".to_string(), "Merge".to_string()]
    ));

    let none = Cli::try_parse_from(["codeterm", "notify", "send", "Done"])
        .expect("choices stay optional");
    assert!(matches!(
        none.command,
        Commands::Notify {
            action: NotifyAction::Send { ref choice, .. }
        } if choice.is_empty()
    ));

    // A label is a value, so a bare `--choice` is a parse error rather than a
    // silent empty choice.
    parse_error(&["codeterm", "notify", "send", "Which branch?", "--choice"]);
}
