use clap::Subcommand;

#[derive(Subcommand)]
pub enum SentinelAction {
    /// Classify a bounded probe or same-Team agent observation
    Create {
        /// Short meaningful name describing what is watched and what happens
        #[arg(long, value_parser = parse_sentinel_name)]
        name: String,
        #[arg(long, conflicts_with = "observe", required_unless_present = "observe")]
        probe: Option<String>,
        /// Observe a same-Team agent tab or cron run
        #[arg(long, conflicts_with = "probe", required_unless_present = "probe", value_parser = parse_observe)]
        observe: Option<String>,
        #[arg(long)]
        question: String,
        #[arg(long, value_delimiter = ',', required = true)]
        buckets: Vec<String>,
        /// Owner-requested cadence (minimum 10s)
        #[arg(long, default_value = "20s")]
        every: String,
        /// Inline routes: bucket=notify|sentinel:<exact-id>|send:<tab>|cron:<name>|stop; use --handlers-file for command, respond_prompt and escalation handlers
        #[arg(long = "route", value_parser = parse_route)]
        routes: Vec<String>,
        /// JSON bucket-to-array of {id,type,parameters} handlers, including command, respond_prompt and escalation on `other`
        #[arg(long, conflicts_with = "routes")]
        handlers_file: Option<std::path::PathBuf>,
        /// Bound how long an `other` escalation may hold this stage, in milliseconds
        #[arg(long)]
        escalation_timeout_ms: Option<u64>,
        /// Create a dormant downstream stage; its lifetime starts now
        #[arg(long)]
        paused: bool,
        /// Finite accepted transition budget (maximum 256)
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=256))]
        max_transitions: Option<u16>,
        /// Finite action-attempt budget (maximum 1024)
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=1024))]
        max_actions: Option<u16>,
        /// Stop after this bucket's first confident transition
        #[arg(long)]
        until: Option<String>,
        /// End the sentinel after this duration
        #[arg(long)]
        timeout: Option<String>,
        /// Deterministic fallback when no Choice adapter is connected: bucket=regex
        #[arg(long = "match", value_parser = parse_assignment)]
        matches: Vec<String>,
        /// Validate and print the resulting graph without creating it
        #[arg(long)]
        dry_run: bool,
    },
    /// List sentinels scoped to the calling tab and its Team
    List,
    /// Show one sentinel
    Show { id: String },
    /// Stop one sentinel
    Stop { id: String },
    /// Replace one definition at the expected version
    Update {
        id: String,
        #[arg(long)]
        expect_version: u64,
        #[arg(long)]
        from_file: std::path::PathBuf,
        #[arg(long)]
        cause: Option<String>,
        /// Validate and print the resulting graph without updating it
        #[arg(long)]
        dry_run: bool,
    },
    /// Show immutable definition versions
    History { id: String },
    /// Resolve an open escalation
    Escalation {
        #[command(subcommand)]
        action: SentinelEscalationAction,
    },
}

#[derive(Subcommand)]
pub enum SentinelEscalationAction {
    /// Release a held stage and re-evaluate its observation
    Resolve { id: String, transition_id: String },
}

fn parse_assignment(value: &str) -> Result<String, String> {
    let (bucket, rhs) = value.split_once('=').ok_or("use bucket=value")?;
    if bucket.is_empty() || rhs.is_empty() {
        return Err("bucket and value must be nonempty".into());
    }
    Ok(value.into())
}

fn parse_sentinel_name(value: &str) -> Result<String, String> {
    codeterm_types::automation_name::validate_sentinel_name(value)?;
    Ok(value.to_string())
}

fn parse_observe(value: &str) -> Result<String, String> {
    let target = value
        .strip_prefix("tab:")
        .or_else(|| value.strip_prefix("cron-run:"))
        .ok_or("use tab:<id> or cron-run:<id>")?;
    if target.trim().is_empty() || target.len() > 256 || target.chars().any(char::is_control) {
        return Err("--observe target must be nonempty and bounded".into());
    }
    Ok(value.into())
}

fn parse_route(value: &str) -> Result<String, String> {
    let value = parse_assignment(value)?;
    let (_, action) = value.split_once('=').ok_or("use bucket=action")?;
    if action == "notify"
        || action == "stop"
        || action
            .strip_prefix("send:")
            .or_else(|| action.strip_prefix("cron:"))
            .or_else(|| action.strip_prefix("sentinel:"))
            .is_some_and(|target| !target.trim().is_empty())
    {
        Ok(value)
    } else {
        Err("route must be notify, sentinel:<exact-id>, send:<tab>, cron:<name>, or stop".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::cli::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn sentinel_create_parse_and_json() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "create",
            "--name",
            "ci-result-watch",
            "--probe",
            "check-ci",
            "--question",
            "CI state?",
            "--buckets",
            "pending,passed,failed",
            "--every",
            "20s",
            "--escalation-timeout-ms",
            "5000",
            "--route",
            "passed=send:Review",
            "--route",
            "failed=stop",
            "--match",
            "passed=^success$",
            "--until",
            "passed",
            "--timeout",
            "1h",
            "--json",
        ])
        .unwrap();
        assert!(cli.json);
        let Commands::Sentinel {
            action:
                SentinelAction::Create {
                    buckets,
                    routes,
                    matches,
                    escalation_timeout_ms,
                    ..
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(buckets, ["pending", "passed", "failed"]);
        assert_eq!(routes.len(), 2);
        assert_eq!(matches, ["passed=^success$"]);
        assert_eq!(escalation_timeout_ms, Some(5000));
        for args in [
            vec!["codeterm", "sentinel", "list", "--json"],
            vec!["codeterm", "sentinel", "show", "id", "--json"],
            vec!["codeterm", "sentinel", "stop", "id", "--json"],
            vec!["codeterm", "sentinel", "history", "id", "--json"],
        ] {
            assert!(Cli::try_parse_from(args).unwrap().json);
        }
    }

    #[test]
    fn sentinel_observe_parse_and_probe_conflict() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "create",
            "--name",
            "agent-state-watch",
            "--observe",
            "tab:bound-agent",
            "--question",
            "tab state?",
            "--buckets",
            "waiting",
            "--match",
            "waiting=.*",
        ])
        .unwrap();
        let Commands::Sentinel {
            action: SentinelAction::Create { probe, observe, .. },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(probe, None);
        assert_eq!(observe.as_deref(), Some("tab:bound-agent"));
        assert!(Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "create",
            "--name",
            "invalid-conflict-watch",
            "--probe",
            "check",
            "--observe",
            "tab:bound-agent",
            "--question",
            "state?",
            "--buckets",
            "waiting",
        ])
        .is_err());
        for invalid in ["cron:check", "cron-run:", "tab:", "tab:bad\nname"] {
            assert!(parse_observe(invalid).is_err());
        }
        assert_eq!(
            parse_observe("cron-run:nightly"),
            Ok("cron-run:nightly".into())
        );
    }

    #[test]
    fn sentinel_update_parse_requires_expect_version_and_file() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "update",
            "sentinel_a",
            "--expect-version",
            "4",
            "--from-file",
            "definition.json",
            "--cause",
            "transition_9",
            "--json",
        ])
        .unwrap();
        let Commands::Sentinel {
            action:
                SentinelAction::Update {
                    id,
                    expect_version,
                    from_file,
                    cause,
                    ..
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(id, "sentinel_a");
        assert_eq!(expect_version, 4);
        assert_eq!(from_file, std::path::PathBuf::from("definition.json"));
        assert_eq!(cause.as_deref(), Some("transition_9"));
        assert!(Cli::try_parse_from(["codeterm", "sentinel", "update", "id"]).is_err());
    }

    #[test]
    fn sentinel_escalate_resolve_parse_requires_stage_and_transition_ids() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "escalation",
            "resolve",
            "sentinel_a",
            "transition_9",
            "--json",
        ])
        .unwrap();
        let Commands::Sentinel {
            action:
                SentinelAction::Escalation {
                    action: SentinelEscalationAction::Resolve { id, transition_id },
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(id, "sentinel_a");
        assert_eq!(transition_id, "transition_9");
        assert!(Cli::try_parse_from([
            "codeterm",
            "sentinel",
            "escalation",
            "resolve",
            "sentinel_a",
        ])
        .is_err());
    }

    #[test]
    fn sentinel_routes_closed_set() {
        for action in [
            "notify",
            "sentinel:sentinel_child",
            "send:tab",
            "cron:check",
            "stop",
        ] {
            assert!(parse_route(&format!("passed={action}")).is_ok());
        }
        for action in ["sh:evil", "send:", "cron:", "notify;evil", "exec", ""] {
            assert!(parse_route(&format!("passed={action}")).is_err());
        }
    }

    #[test]
    fn sentinel_create_requires_a_short_non_raw_name() {
        let base = [
            "codeterm", "sentinel", "create", "--probe", "status", "--question", "state?",
            "--buckets", "done",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        for name in ["ab", "sentinel_5d22889a8898"] {
            assert!(Cli::try_parse_from([
                "codeterm", "sentinel", "create", "--name", name, "--probe", "status",
                "--question", "state?", "--buckets", "done",
            ]).is_err());
        }
        assert!(Cli::try_parse_from([
            "codeterm", "sentinel", "create", "--name", "ci-green-watch", "--probe", "status",
            "--question", "state?", "--buckets", "done",
        ]).is_ok());
    }

    #[test]
    fn sentinel_composition_flags_refuse_mixed_input_and_out_of_range_budgets() {
        let prefix = [
            "codeterm",
            "sentinel",
            "create",
            "--name",
            "budget-validation-watch",
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
        ];
        for suffix in [
            vec!["--route", "done=notify", "--handlers-file", "handlers.json"],
            vec!["--max-transitions", "0"],
            vec!["--max-transitions", "257"],
            vec!["--max-actions", "0"],
            vec!["--max-actions", "1025"],
        ] {
            assert!(Cli::try_parse_from(prefix.into_iter().chain(suffix)).is_err());
        }
    }
}
