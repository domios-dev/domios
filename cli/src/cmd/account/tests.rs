use crate::args::account::AccountAction;
use crate::{Cli, Commands};
use clap::Parser;
#[test]
fn no_browser_login_accepts_stdin_or_pad_and_pad_requires_no_browser() {
    for args in [
        vec!["domios", "account", "login", "--no-browser"],
        vec![
            "domios",
            "account",
            "login",
            "--no-browser",
            "--code-from-pad",
            "callback.txt",
        ],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Account {
                action: AccountAction::Login {
                    no_browser: true,
                    ..
                }
            }
        ));
    }
    assert!(Cli::try_parse_from([
        "domios",
        "account",
        "login",
        "--code-from-pad",
        "callback.txt"
    ])
    .is_err());
}
#[test]
fn account_actions_parse_and_are_local() {
    assert!(Cli::try_parse_from(["codeterm", "account", "upgrade", "pro"]).is_ok());
    assert!(Cli::try_parse_from(["codeterm", "account", "upgrade"]).is_err());
    for action in ["status", "login", "logout", "plan", "portal"] {
        let cli = Cli::try_parse_from(["codeterm", "account", action]).unwrap();
        assert!(matches!(cli.command, Commands::Account { .. }));
        assert_eq!(
            crate::transport::transport_for(&cli.command).local,
            crate::transport::Local::PreSocket
        );
    }
}
