//! `codeterm sessions backups` verb contracts: parse shape for desk-backup
//! list/restore, plus regression that extending the enum kept existing verbs.

use crate::args::sessions::{BackupsAction, SessionsAction};
use clap::Parser;

#[derive(Parser)]
struct SessionsOnly {
    #[command(subcommand)]
    action: SessionsAction,
}

#[test]
fn sessions_backups_list_parses_with_default_limit() {
    let parsed =
        SessionsOnly::try_parse_from(["sessions", "backups", "list"]).expect("backups list parses");
    match parsed.action {
        SessionsAction::Backups {
            action: BackupsAction::List { limit },
        } => assert_eq!(limit, 20),
        _ => panic!("expected Backups/List"),
    }
}

#[test]
fn sessions_backups_list_accepts_limit() {
    let parsed = SessionsOnly::try_parse_from(["sessions", "backups", "list", "--limit", "3"])
        .expect("backups list --limit parses");
    match parsed.action {
        SessionsAction::Backups {
            action: BackupsAction::List { limit },
        } => assert_eq!(limit, 3),
        _ => panic!("expected Backups/List"),
    }
}

#[test]
fn sessions_backups_restore_requires_a_backup_name() {
    assert!(
        SessionsOnly::try_parse_from(["sessions", "backups", "restore"]).is_err(),
        "restore without a name must be a parse error, never an implicit pick"
    );
}

#[test]
fn sessions_backups_restore_parses_name() {
    let parsed = SessionsOnly::try_parse_from(["sessions", "backups", "restore", "1754700000"])
        .expect("backups restore NAME parses");
    match parsed.action {
        SessionsAction::Backups {
            action: BackupsAction::Restore { name },
        } => assert_eq!(name, "1754700000"),
        _ => panic!("expected Backups/Restore"),
    }
}

#[test]
fn sessions_backups_without_subaction_is_an_error() {
    assert!(
        SessionsOnly::try_parse_from(["sessions", "backups"]).is_err(),
        "list vs restore must be explicit"
    );
}

#[test]
fn existing_sessions_verbs_still_parse() {
    assert!(SessionsOnly::try_parse_from(["sessions", "search", "query terms"]).is_ok());
    assert!(SessionsOnly::try_parse_from(["sessions", "get", "some-session-id"]).is_ok());
    assert!(SessionsOnly::try_parse_from(["sessions", "resume", "some-session-id"]).is_ok());
}
