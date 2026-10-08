use clap::Subcommand;

#[derive(Subcommand)]
pub enum BlueprintAction {
    /// List built-in graph skeletons or print one ready-to-edit recipe
    Recipe { name: Option<String> },
    /// Classify a bounded probe or same-Team agent observation
    #[command(group = clap::ArgGroup::new("input_source").args(["input_file", "input_url", "input_text"]).multiple(false).required(false))]
    Create {
        /// JSON or YAML pipeline specification, or `-` to read standard input
        #[arg(long, conflicts_with = "recipe")]
        spec: Option<std::path::PathBuf>,
        /// Create from a named pipeline recipe
        #[arg(long, conflicts_with = "spec")]
        recipe: Option<String>,
        /// Replace recipe placeholders with key=value pairs
        #[arg(long = "set", value_parser = parse_assignment)]
        set: Vec<String>,
        /// Assign the pipeline to its creator tab or Team
        #[arg(long, value_parser = ["tab", "team"])]
        owner: Option<String>,
        /// Short meaningful name describing what is watched and what happens
        #[arg(long, value_parser = parse_blueprint_name, required_unless_present_any = ["spec", "recipe"])]
        name: Option<String>,
        #[arg(long, conflicts_with = "observe", required_unless_present_any = ["observe", "input", "spec", "recipe"])]
        probe: Option<String>,
        /// Observe a same-Team tab or cron run; shells require terminal-pattern or output-settled
        #[arg(long, conflicts_with = "probe", required_unless_present_any = ["probe", "input", "spec", "recipe"], value_parser = parse_observe)]
        observe: Option<String>,
        /// Fire with the new terminal output delta when this regex matches
        #[arg(long, requires = "observe")]
        terminal_pattern: Option<String>,
        /// Fire when terminal output settles for this many milliseconds (10..60000)
        #[arg(long, requires = "observe", value_parser = clap::value_parser!(u64).range(10..=60000))]
        output_settled: Option<u64>,
        /// Keep exchanges outside transition/action budgets; stop at 16384 exchanges or 120/minute
        #[arg(long)]
        session: bool,
        /// Watch data instead of a probe or observation: text|audio|video|image
        #[arg(long, value_parser = parse_input_kind, requires = "input_source", conflicts_with_all = ["probe", "observe", "every", "rearm_on_change"])]
        input: Option<String>,
        /// Absolute or caller-cwd-relative path of the input data file
        #[arg(long, value_name = "PATH", requires = "input", conflicts_with_all = ["probe", "observe", "every", "rearm_on_change"])]
        input_file: Option<std::path::PathBuf>,
        /// http or https URL of the input data
        #[arg(long, requires = "input", conflicts_with_all = ["probe", "observe", "every", "rearm_on_change"], value_parser = parse_input_url)]
        input_url: Option<String>,
        /// Inline input text (only for --input text)
        #[arg(long, requires = "input", conflicts_with_all = ["probe", "observe", "every", "rearm_on_change"])]
        input_text: Option<String>,
        /// Convert the input to another kind once: text|audio
        #[arg(long, value_name = "KIND", requires = "input", conflicts_with_all = ["probe", "observe", "every", "rearm_on_change"], value_parser = parse_convert_to)]
        convert_to: Option<String>,
        /// Bound the one-time conversion, in the --every duration syntax
        #[arg(long, value_name = "DURATION", requires = "input", conflicts_with_all = ["probe", "observe", "every", "rearm_on_change"], value_parser = parse_convert_timeout)]
        convert_timeout: Option<u64>,
        #[arg(long, required_unless_present_any = ["spec", "recipe"])]
        question: Option<String>,
        #[arg(long, value_delimiter = ',', required_unless_present_any = ["spec", "recipe"])]
        buckets: Option<Vec<String>>,
        /// Owner-requested cadence (minimum 10s, default 20s)
        #[arg(long)]
        every: Option<String>,
        /// Inline routes: bucket=notify|blueprint:<exact-id>|send:<tab>|cron:<name>|stop|none; use --handlers-file for command, respond_prompt and escalation handlers
        #[arg(long = "route", value_parser = parse_route)]
        routes: Vec<String>,
        /// Route again when this bucket's output changes without changing buckets
        #[arg(long = "rearm-on-change", value_name = "BUCKET")]
        rearm_on_change: Vec<String>,
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
        /// End the blueprint after this duration
        #[arg(long)]
        timeout: Option<String>,
        /// Deterministic fallback when no Choice adapter is connected: bucket=regex
        #[arg(long = "match", value_parser = parse_assignment)]
        matches: Vec<String>,
        /// Validate and print the resulting graph without creating it
        #[arg(long)]
        dry_run: bool,
    },
    /// Export a portable recipe containing only the definition
    Export {
        id: String,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Import a portable recipe into the caller's Team (stopped by default)
    Import {
        file: std::path::PathBuf,
        #[arg(long = "set", value_parser = parse_assignment)]
        set: Vec<String>,
        #[arg(long, conflicts_with = "start")]
        stopped: bool,
        /// Start immediately after validation
        #[arg(long, conflicts_with = "stopped")]
        start: bool,
    },
    /// List trigger, step, converter and event-source capabilities
    Nodes,
    /// List blueprints scoped to the calling tab and its Team
    List,
    /// Show one blueprint
    Show { id: String },
    /// Deliver a text body to a running text input stage
    #[command(group = clap::ArgGroup::new("text_source").args(["from_pad", "text_file"]).required(true))]
    Input {
        id: String,
        /// Read a UTF-8 body from the caller's Agent Pad
        #[arg(long)]
        from_pad: Option<String>,
        /// Read a UTF-8 body from a file
        #[arg(long)]
        text_file: Option<std::path::PathBuf>,
    },
    /// Stop one blueprint
    Stop { id: String },
    /// Delete a whole blueprint, or every stopped one in the Team; the creator or the Team owner may delete
    #[command(group = clap::ArgGroup::new("delete_target").args(["id", "stopped"]).required(true))]
    Delete {
        id: Option<String>,
        /// Delete every stopped blueprint the caller may delete in its Team
        #[arg(long, conflicts_with_all = ["id", "stop"])]
        stopped: bool,
        /// Stop a running blueprint before deleting it
        #[arg(long, requires = "id")]
        stop: bool,
    },
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
    /// List bounded run journal entries, newest first
    Log {
        id: String,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=200))]
        limit: u16,
    },
    /// Resolve an open escalation
    Escalation {
        #[command(subcommand)]
        action: BlueprintEscalationAction,
    },
}

#[derive(Subcommand)]
pub enum BlueprintEscalationAction {
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

fn parse_blueprint_name(value: &str) -> Result<String, String> {
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

fn parse_input_kind(value: &str) -> Result<String, String> {
    codeterm_types::sentinel_input::MediaKind::parse(value)
        .map(|kind| kind.as_str().to_string())
        .ok_or_else(|| "--input: use text, audio, video or image".into())
}

fn parse_convert_to(value: &str) -> Result<String, String> {
    match value {
        "text" | "audio" => Ok(value.into()),
        _ => Err("--convert-to: use text or audio".into()),
    }
}

fn parse_input_url(value: &str) -> Result<String, String> {
    use codeterm_types::sentinel_input::{validate, DataRef, InputSpec, MediaKind};
    let probe = InputSpec {
        kind: MediaKind::Audio,
        data: DataRef::Url(value.to_string()),
        convert_to: None,
        convert_timeout_ms: None,
    };
    validate(&probe).map_err(|error| format!("--input-url: {error}"))?;
    Ok(value.into())
}

fn parse_convert_timeout(value: &str) -> Result<u64, String> {
    if value.len() > 32
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 's' | 'm' | 'h' | 'd'))
    {
        return Err("duration: use seconds, minutes, hours or days, e.g. 30s or 15m".into());
    }
    if value.chars().any(|c| c.is_ascii_alphabetic())
        && value.ends_with(|c: char| c.is_ascii_digit())
    {
        return Err("duration: every component needs a unit".into());
    }
    let mut number = String::new();
    let mut bound = 0u64;
    for c in value.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().map_err(|_| "duration: invalid number")?;
        number.clear();
        let factor = match c {
            's' => 1000,
            'm' => 60_000,
            'h' => 3_600_000,
            'd' => 86_400_000,
            _ => unreachable!(),
        };
        bound = bound
            .checked_add(n.checked_mul(factor).ok_or("duration: overflow")?)
            .ok_or("duration: overflow")?;
    }
    if !number.is_empty() {
        bound = bound
            .checked_add(
                number
                    .parse::<u64>()
                    .map_err(|_| "duration: invalid number")?
                    .checked_mul(60_000)
                    .ok_or("duration: overflow")?,
            )
            .ok_or("duration: overflow")?;
    }
    if bound == 0 {
        return Err("duration: must be positive".into());
    }
    Ok(bound)
}

fn parse_route(value: &str) -> Result<String, String> {
    let value = parse_assignment(value)?;
    let (_, action) = value.split_once('=').ok_or("use bucket=action")?;
    if action == "none"
        || action == "notify"
        || action == "stop"
        || action
            .strip_prefix("send:")
            .or_else(|| action.strip_prefix("cron:"))
            .or_else(|| action.strip_prefix("blueprint:"))
            .is_some_and(|target| !target.trim().is_empty())
    {
        Ok(value)
    } else {
        Err(
            "route must be notify, blueprint:<exact-id>, send:<tab>, cron:<name>, stop, or none"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::cli::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn blueprint_rejects_removed_command_and_route() {
        assert!(Cli::try_parse_from(["codeterm", "sentinel", "list"]).is_err());
        assert!(parse_route("done=sentinel:exact-id").is_err());
        assert_eq!(
            parse_route("done=blueprint:exact-id").unwrap(),
            "done=blueprint:exact-id"
        );
    }

    #[test]
    fn sentinel_create_parse_and_json() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "blueprint",
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
            "--route",
            "pending=none",
            "--rearm-on-change",
            "pending",
            "--rearm-on-change",
            "failed",
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
        let Commands::Blueprint {
            action:
                BlueprintAction::Create {
                    buckets,
                    routes,
                    rearm_on_change,
                    matches,
                    escalation_timeout_ms,
                    ..
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(buckets.unwrap(), ["pending", "passed", "failed"]);
        assert_eq!(routes.len(), 3);
        assert_eq!(rearm_on_change, ["pending", "failed"]);
        assert_eq!(matches, ["passed=^success$"]);
        assert_eq!(escalation_timeout_ms, Some(5000));
        for args in [
            vec!["codeterm", "blueprint", "list", "--json"],
            vec!["codeterm", "blueprint", "show", "id", "--json"],
            vec!["codeterm", "blueprint", "stop", "id", "--json"],
            vec!["codeterm", "blueprint", "history", "id", "--json"],
        ] {
            assert!(Cli::try_parse_from(args).unwrap().json);
        }
    }

    #[test]
    fn sentinel_observe_parse_and_probe_conflict() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "blueprint",
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
        let Commands::Blueprint {
            action: BlueprintAction::Create { probe, observe, .. },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(probe, None);
        assert_eq!(observe.as_deref(), Some("tab:bound-agent"));
        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
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
            "blueprint",
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
        let Commands::Blueprint {
            action:
                BlueprintAction::Update {
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
        assert!(Cli::try_parse_from(["codeterm", "blueprint", "update", "id"]).is_err());
    }

    #[test]
    fn sentinel_escalate_resolve_parse_requires_stage_and_transition_ids() {
        let cli = Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "escalation",
            "resolve",
            "sentinel_a",
            "transition_9",
            "--json",
        ])
        .unwrap();
        let Commands::Blueprint {
            action:
                BlueprintAction::Escalation {
                    action: BlueprintEscalationAction::Resolve { id, transition_id },
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(id, "sentinel_a");
        assert_eq!(transition_id, "transition_9");
        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
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
            "blueprint:sentinel_child",
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
            "codeterm",
            "blueprint",
            "create",
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        for name in ["ab", "sentinel_5d22889a8898"] {
            assert!(Cli::try_parse_from([
                "codeterm",
                "blueprint",
                "create",
                "--name",
                name,
                "--probe",
                "status",
                "--question",
                "state?",
                "--buckets",
                "done",
            ])
            .is_err());
        }
        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--name",
            "ci-green-watch",
            "--probe",
            "status",
            "--question",
            "state?",
            "--buckets",
            "done",
        ])
        .is_ok());
    }

    #[test]
    fn sentinel_composition_flags_refuse_mixed_input_and_out_of_range_budgets() {
        let prefix = [
            "codeterm",
            "blueprint",
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

    fn input_prefix() -> Vec<&'static str> {
        vec![
            "codeterm",
            "blueprint",
            "create",
            "--name",
            "voice-note-watch",
            "--input",
            "audio",
            "--question",
            "transcribed?",
            "--buckets",
            "done",
        ]
    }

    #[test]
    fn sentinel_input_flags_parse_and_conflict() {
        let cli = Cli::try_parse_from(
            input_prefix()
                .into_iter()
                .chain([
                    "--input-file",
                    "clips/voice.wav",
                    "--convert-to",
                    "text",
                    "--convert-timeout",
                    "30s",
                ])
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let Commands::Blueprint {
            action:
                BlueprintAction::Create {
                    input,
                    input_file,
                    input_url,
                    input_text,
                    convert_to,
                    convert_timeout,
                    ..
                },
        } = cli.command
        else {
            panic!("wrong command")
        };
        assert_eq!(input.as_deref(), Some("audio"));
        assert_eq!(
            input_file.as_deref(),
            Some(std::path::Path::new("clips/voice.wav"))
        );
        assert_eq!(input_url, None);
        assert_eq!(input_text, None);
        assert_eq!(convert_to.as_deref(), Some("text"));
        assert_eq!(convert_timeout, Some(30_000));

        for invalid in [
            vec!["--input", "audio", "--probe", "status"],
            vec!["--input", "audio", "--observe", "tab:bound"],
            vec![
                "--input-file",
                "clips/voice.wav",
                "--input-url",
                "https://e.com/a",
            ],
            vec!["--input-file", "clips/voice.wav", "--input-text", "hello"],
            vec!["--input", "audio"],
            vec!["--input", "midi", "--input-file", "clips/voice.wav"],
            vec!["--input", "audio", "--convert-to", "video"],
            vec!["--input", "audio", "--input-url", "ftp://example.com/a"],
            vec!["--input", "audio", "--convert-timeout", "forever"],
            vec![
                "--input",
                "audio",
                "--input-file",
                "clips/voice.wav",
                "--rearm-on-change",
                "done",
            ],
        ] {
            assert!(
                Cli::try_parse_from(input_prefix().into_iter().chain(invalid.iter().copied()))
                    .is_err(),
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn sentinel_input_every_conflicts_only_when_explicit() {
        assert!(Cli::try_parse_from(input_prefix().into_iter().chain([
            "--input-file",
            "clips/voice.wav",
            "--every",
            "20s"
        ]))
        .is_err());
        assert!(Cli::try_parse_from(
            input_prefix()
                .into_iter()
                .chain(["--input-file", "clips/voice.wav"])
        )
        .is_ok());
    }

    #[test]
    fn blueprint_pipeline_create_nodes_and_log_parse() {
        let spec = Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--spec",
            "pipeline.yaml",
            "--owner",
            "team",
            "--dry-run",
        ])
        .unwrap();
        let Commands::Blueprint {
            action:
                BlueprintAction::Create {
                    spec,
                    owner,
                    dry_run,
                    ..
                },
        } = spec.command
        else {
            panic!("wrong command")
        };
        assert_eq!(spec, Some(std::path::PathBuf::from("pipeline.yaml")));
        assert_eq!(owner.as_deref(), Some("team"));
        assert!(dry_run);

        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "create",
            "--recipe",
            "echo",
            "--set",
            "message=ready"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["codeterm", "blueprint", "nodes", "--json"]).is_ok());
        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "log",
            "blueprint-id",
            "--limit",
            "200",
            "--json"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "codeterm",
            "blueprint",
            "log",
            "blueprint-id",
            "--limit",
            "201"
        ])
        .is_err());
    }
}
