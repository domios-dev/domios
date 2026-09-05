use super::*;

#[test]
fn tab_repair_parses_each_evidence_mode_and_target() {
    let defaulted =
        Cli::try_parse_from(["codeterm", "tab", "repair"]).expect("default evidence repair parses");
    assert!(matches!(
        defaulted.command,
        Commands::Tab {
            action: TabAction::Repair {
                provider: None,
                session: None,
                from_evidence: false,
                from_journal: false,
                tab: None,
            },
        }
    ));

    let live = Cli::try_parse_from(["codeterm", "tab", "repair", "--from-evidence"])
        .expect("live-evidence repair parses");
    assert!(matches!(
        live.command,
        Commands::Tab {
            action: TabAction::Repair {
                provider: None,
                session: None,
                from_evidence: true,
                from_journal: false,
                tab: None,
            },
        }
    ));

    let journal = Cli::try_parse_from([
        "codeterm",
        "tab",
        "repair",
        "--from-journal",
        "--tab",
        "tab-a",
    ])
    .expect("journal repair parses");
    assert!(matches!(
        journal.command,
        Commands::Tab {
            action: TabAction::Repair {
                from_journal: true,
                tab: Some(ref tab),
                ..
            },
        } if tab == "tab-a"
    ));

    let provider = Cli::try_parse_from(["codeterm", "tab", "repair", "--provider", "codex"])
        .expect("provider assertion parses");
    assert!(matches!(
        provider.command,
        Commands::Tab {
            action: TabAction::Repair {
                provider: Some(ref provider),
                ..
            },
        } if provider == "codex"
    ));

    let session = Cli::try_parse_from(["codeterm", "tab", "repair", "--session", "session-live"])
        .expect("session assertion parses");
    assert!(matches!(
        session.command,
        Commands::Tab {
            action: TabAction::Repair {
                session: Some(ref session),
                ..
            },
        } if session == "session-live"
    ));
}

#[test]
fn tab_repair_modes_are_mutually_exclusive() {
    let error = Cli::try_parse_from([
        "codeterm",
        "tab",
        "repair",
        "--provider",
        "codex",
        "--session",
        "session-live",
    ])
    .expect_err("repair cannot combine assertion modes");

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}
