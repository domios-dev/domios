use clap::Parser;
use crate::{Cli, Commands, MemAction};

#[test]
fn pin_subcommand_parses_id() {
    let cli = Cli::try_parse_from(["codeterm", "mem", "pin", "fact_abc"]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Pin { id } } => assert_eq!(id, "fact_abc"),
        _ => panic!("expected Pin"),
    }
}

#[test]
fn unpin_subcommand_parses_id() {
    let cli = Cli::try_parse_from(["codeterm", "mem", "unpin", "fact_abc"]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Unpin { id } } => assert_eq!(id, "fact_abc"),
        _ => panic!("expected Unpin"),
    }
}

#[test]
fn pinned_subcommand_parses_with_limit() {
    let cli = Cli::try_parse_from(["codeterm", "mem", "pinned", "--limit", "10"]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Pinned { limit } } => assert_eq!(limit, Some(10)),
        _ => panic!("expected Pinned"),
    }
}

#[test]
fn pinned_subcommand_no_limit_is_none() {
    let cli = Cli::try_parse_from(["codeterm", "mem", "pinned"]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Pinned { limit } } => assert_eq!(limit, None),
        _ => panic!("expected Pinned"),
    }
}

#[test]
fn save_accepts_pin_flag() {
    let cli = Cli::try_parse_from([
        "codeterm","mem","save","--kind","fact","--title","t","--pin","body"
    ]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Save { pin, .. } } => assert!(pin),
        _ => panic!("expected Save"),
    }
}

#[test]
fn save_accepts_workspace_flag() {
    let cli = Cli::try_parse_from([
        "codeterm","mem","save","--kind","fact","--title","t","--workspace","W1","body"
    ]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Save { workspace, .. } } => {
            assert_eq!(workspace.as_deref(), Some("W1"));
        }
        _ => panic!("expected Save"),
    }
}

#[test]
fn wakeup_accepts_explain_flag() {
    let cli = Cli::try_parse_from(["codeterm","mem","wake-up","--explain"]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::WakeUp { explain, .. } } => assert!(explain),
        _ => panic!("expected WakeUp"),
    }
}

#[test]
fn wakeup_no_explain_default_false() {
    let cli = Cli::try_parse_from(["codeterm","mem","wake-up"]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::WakeUp { explain, .. } } => assert!(!explain),
        _ => panic!("expected WakeUp"),
    }
}

#[test]
fn update_accepts_changed_by_flag() {
    let cli = Cli::try_parse_from([
        "codeterm","mem","update","fact_a","--changed-by","dreamer-calibration"
    ]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Update { changed_by, .. } } => {
            assert_eq!(changed_by.as_deref(), Some("dreamer-calibration"));
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn update_without_changed_by_is_none() {
    let cli = Cli::try_parse_from([
        "codeterm","mem","update","fact_a","--importance","4.0"
    ]).unwrap();
    match cli.command {
        Commands::Mem { action: MemAction::Update { changed_by, .. } } => {
            assert!(changed_by.is_none());
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn calibration_log_subcommand_parses() {
    use crate::MemCalibrationAction;
    let cli = Cli::try_parse_from([
        "codeterm","mem","calibration","log","--limit","10","--count"
    ]).unwrap();
    match cli.command {
        Commands::Mem {
            action: MemAction::Calibration {
                action: MemCalibrationAction::Log { limit, count, .. },
            },
        } => {
            assert_eq!(limit, Some(10));
            assert!(count);
        }
        _ => panic!("expected calibration log"),
    }
}

#[test]
fn calibration_sweep_subcommand_parses() {
    use crate::MemCalibrationAction;
    let cli = Cli::try_parse_from([
        "codeterm","mem","calibration","sweep","--force"
    ]).unwrap();
    match cli.command {
        Commands::Mem {
            action: MemAction::Calibration {
                action: MemCalibrationAction::Sweep { force },
            },
        } => {
            assert!(force);
        }
        _ => panic!("expected calibration sweep"),
    }
}

#[test]
fn calibration_sweep_no_force_defaults_false() {
    use crate::MemCalibrationAction;
    let cli = Cli::try_parse_from([
        "codeterm","mem","calibration","sweep"
    ]).unwrap();
    match cli.command {
        Commands::Mem {
            action: MemAction::Calibration {
                action: MemCalibrationAction::Sweep { force },
            },
        } => {
            assert!(!force);
        }
        _ => panic!("expected calibration sweep"),
    }
}
