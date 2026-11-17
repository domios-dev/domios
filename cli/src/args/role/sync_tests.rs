use super::RoleAction;
use crate::args::cli::{Cli, Commands};
use clap::Parser;

#[test]
fn role_sync_requires_an_explicit_mode_and_apply_hashes() {
    let prefix = ["domios", "role", "sync", "curator", "--team", "team_a"];
    let parse =
        |flags: &[&str]| Cli::try_parse_from(prefix.into_iter().chain(flags.iter().copied()));
    assert!(matches!(
        parse(&["--preview"]).unwrap().command,
        Commands::Role {
            action: RoleAction::Sync { preview: true, .. }
        }
    ));
    assert!(matches!(
        parse(&[
            "--apply",
            "--current-hash",
            "old",
            "--replacement-hash",
            "new"
        ])
        .unwrap()
        .command,
        Commands::Role {
            action: RoleAction::Sync { apply: true, .. }
        }
    ));
    for flags in [
        vec![],
        vec!["--apply"],
        vec!["--yes"],
        vec!["--preview", "--apply"],
        vec!["--apply", "--current-hash", "old"],
    ] {
        assert!(parse(&flags).is_err(), "{flags:?}");
    }
}
