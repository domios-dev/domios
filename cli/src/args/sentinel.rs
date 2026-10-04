use clap::Subcommand;

#[derive(Subcommand)]
pub enum SentinelAction {
    /// Classify a bounded probe or same-Team agent tab observation
    Create {
        /// Display name of the Sentinel scenario or stage
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "observe", required_unless_present = "observe")]
        probe: Option<String>,
        /// Observe a same-Team agent tab and wake on prompt or status changes
        #[arg(long, conflicts_with = "probe", required_unless_present = "probe", value_parser = parse_observe)]
        observe: Option<String>,
        #[arg(long)]
        question: String,
        #[arg(long, value_delimiter = ',', required = true)]
        buckets: Vec<String>,
        /// Owner-requested cadence (minimum 10s)
        #[arg(long, default_value = "20s")]
        every: String,
        /// Closed-set route: bucket=notify|sentinel:<exact-id>|send:<tab>|cron:<name>|stop
        #[arg(long = "route", value_parser = parse_route)]
        routes: Vec<String>,
        /// JSON bucket-to-array of {id,type,parameters} handlers, including fixed local command actions
        #[arg(long, conflicts_with = "routes")]
        handlers_file: Option<std::path::PathBuf>,
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
    },
    /// List sentinels scoped to the calling tab and its Team
    List,
    /// Show one sentinel
    Show { id: String },
    /// Stop one sentinel
    Stop { id: String },
}

fn parse_assignment(value: &str) -> Result<String, String> {
    let (bucket, rhs) = value.split_once('=').ok_or("use bucket=value")?;
    if bucket.is_empty() || rhs.is_empty() {
        return Err("bucket and value must be nonempty".into());
    }
    Ok(value.into())
}

fn parse_observe(value: &str) -> Result<String, String> {
    let tab_id = value.strip_prefix("tab:").ok_or("use tab:<id>")?;
    if tab_id.trim().is_empty() || tab_id.len() > 256 || tab_id.chars().any(char::is_control) {
        return Err("--observe: tab id must be nonempty and bounded".into());
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
            "--probe",
            "check-ci",
            "--question",
            "CI state?",
            "--buckets",
            "pending,passed,failed",
            "--every",
            "20s",
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
                    ..
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(buckets, ["pending", "passed", "failed"]);
        assert_eq!(routes.len(), 2);
        assert_eq!(matches, ["passed=^success$"]);
        for args in [
            vec!["codeterm", "sentinel", "list", "--json"],
            vec!["codeterm", "sentinel", "show", "id", "--json"],
            vec!["codeterm", "sentinel", "stop", "id", "--json"],
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
        for invalid in ["cron:check", "tab:", "tab:bad\nname"] {
            assert!(parse_observe(invalid).is_err());
        }
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
    fn sentinel_composition_flags_refuse_mixed_input_and_out_of_range_budgets() {
        let prefix = [
            "codeterm",
            "sentinel",
            "create",
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
