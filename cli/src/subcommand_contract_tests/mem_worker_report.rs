use super::*;
use crate::args::mem::MemWorkerAction;

#[test]
fn dream_worker_report_keeps_summary_for_success_and_partial_finalization() {
    let prompt = include_str!("../../../src-tauri/prompts/dreaming_rem.md")
        .replace("<summary of confirmed work>", "opaque summary")
        .replace("<diary_id>", "diary_opaque")
        .replace("<what did not confirm>", "opaque failure");
    let commands: Vec<_> = prompt
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("domios mem worker report "))
        .collect();
    assert_eq!(commands.len(), 2);
    let token = regex::Regex::new(r#""[^"]*"|\S+"#).unwrap();
    for (command, (outcome, reason)) in commands
        .into_iter()
        .zip([("succeeded", None), ("partial", Some("opaque failure"))])
    {
        let argv = token
            .find_iter(command)
            .map(|value| value.as_str().trim_matches('"'));
        let parsed = Cli::try_parse_from(argv).unwrap();
        let Commands::Mem {
            action:
                MemAction::Worker {
                    action:
                        MemWorkerAction::Report {
                            summary,
                            diary,
                            finalization,
                            reason: parsed_reason,
                        },
                },
        } = parsed.command
        else {
            panic!("expected memory worker report");
        };
        assert_eq!(summary, "opaque summary");
        assert_eq!(diary.as_deref(), Some("diary_opaque"));
        assert_eq!(finalization.as_deref(), Some(outcome));
        assert_eq!(parsed_reason.as_deref(), reason);
    }

    let missing_summary = Cli::try_parse_from([
        "domios",
        "mem",
        "worker",
        "report",
        "--diary",
        "diary_opaque",
        "--finalization",
        "partial",
        "--reason",
        "opaque failure",
    ]);
    let Err(error) = missing_summary else {
        panic!("memory worker reports require their summary");
    };
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn agent_report_body_sources_need_no_positional_summary() {
    for (flag, value) in [
        ("--text", "opaque summary"),
        ("--from-pad", "report.md"),
        ("--doc", "report.md"),
    ] {
        let parsed = Cli::try_parse_from(["domios", "agent", "report", flag, value]).unwrap();
        let Commands::Agent {
            action: AgentAction::Report(args),
        } = parsed.command
        else {
            panic!("expected agent report");
        };
        assert!(args.message.is_none());
        assert_eq!(
            args.text.or(args.from_pad).or(args.doc).as_deref(),
            Some(value)
        );
    }
}
