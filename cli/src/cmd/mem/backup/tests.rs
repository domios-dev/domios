use crate::{Cli, Commands};
use clap::Parser;

#[test]
fn backup_commands_are_local_and_distinct_from_item_restore() {
    for args in [
        vec!["domios", "mem", "backup", "create", "--out", "archive"],
        vec!["domios", "mem", "backup", "restore", "--from", "archive"],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Mem {
                action: crate::MemAction::Backup { .. }
            }
        ));
        assert_eq!(
            crate::transport::transport_for(&cli.command).local,
            crate::transport::Local::PreSocket
        );
        assert!(matches!(
            crate::transport::transport_for(&cli.command).api_verdict,
            crate::transport::ApiVerdict::Unsupported(_)
        ));
    }
    assert!(Cli::try_parse_from(["domios", "mem", "backup", "create"]).is_err());
    assert!(Cli::try_parse_from(["domios", "mem", "backup", "restore"]).is_err());
    assert!(Cli::try_parse_from(["domios", "mem", "restore", "fact_a"]).is_ok());
    assert!(Cli::try_parse_from(["domios", "mem", "restore", "--from", "archive"]).is_err());
}
