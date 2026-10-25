use crate::Cli;
use clap::Parser;

#[test]
fn takeover_needs_confirmation_and_authored_text_stays_in_pad_files() {
    assert!(Cli::try_parse_from(["domios", "account", "neighbors", "enroll"]).is_err());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "enroll",
        "--confirm-takeover"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "send",
        "pair",
        "--body",
        "inline"
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "send",
        "pair",
        "--from-pad",
        "request.md"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "grant",
        "pair",
        "--from-pad",
        "scope.md"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "neighbors",
        "send",
        "pair",
        "--from-pad",
        "request.md",
        "--kind",
        "execute"
    ])
    .is_err());
}
