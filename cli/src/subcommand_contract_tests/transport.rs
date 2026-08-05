//! Transport-declaration coverage. Owned by the transport lane (task_6eyp5a);
//! pre-created here so two live lanes never edit the shared test root.

use clap::{CommandFactory, Parser};

use crate::mesh_cli::api_map::{cli_command_to_api, UNSUPPORTED_OVER_MESH};
use crate::transport::{transport_for, ApiVerdict, Local, MeshRelay};
use crate::{Cli, Commands};

const TEAM_LOCAL_ONLY: &str = "team management is local-only — run it on the target machine";
const TASK_LOCAL_ONLY: &str = "task management is local-only — run it on the target machine";

/// Frozen at 16b1bd46c before any transport edit; one row per `Commands` variant.
/// Column 0 is the clap name, so coverage can be compared against the parser.
const FROZEN: &[(&str, &[&str], Local, MeshRelay, ApiVerdict)] = &[
    (
        "identify",
        &["codeterm", "identify"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "capabilities",
        &["codeterm", "capabilities"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported("capabilities is socket-only; run it on the target machine"),
    ),
    (
        "pad",
        &["codeterm", "pad", "path"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported("pad is caller-local and cannot target a mesh peer"),
    ),
    (
        "plugin",
        &["codeterm", "plugin", "list"],
        Local::PreMesh,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported("plugin management is local-only — run it on the target machine"),
    ),
    (
        "cli-manifest",
        &["codeterm", "cli-manifest"],
        Local::PreSocket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported(
            "cli-manifest is local-only — it dumps this binary's own command tree",
        ),
    ),
    (
        "workspace",
        &["codeterm", "workspace", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "team",
        &["codeterm", "team", "list"],
        Local::Socket,
        MeshRelay::SocketRpc,
        ApiVerdict::Unsupported(TEAM_LOCAL_ONLY),
    ),
    (
        "orchestrator",
        &["codeterm", "orchestrator", "status"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported(
            "orchestrator handoff is local-only — run it on the target machine",
        ),
    ),
    (
        "persona",
        &["codeterm", "persona", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported(
            "persona inspection is socket-only and answers for that daemon's own overlay directory — run it on the target machine",
        ),
    ),
    (
        "tab",
        &["codeterm", "tab", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "send",
        &["codeterm", "send", "hi"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "delayed",
        &["codeterm", "delayed", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "web",
        &["codeterm", "web", "link"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported(
            "web link is local-only — each peer's link needs that peer's own token; run it there",
        ),
    ),
    (
        "paste",
        &["codeterm", "paste", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "notify",
        &["codeterm", "notify", "send", "title"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "keepawake",
        &["codeterm", "keepawake", "status"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "theme",
        &["codeterm", "theme", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "agent",
        &["codeterm", "agent", "providers"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "cron",
        &["codeterm", "cron", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "service",
        &["codeterm", "service", "list"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "update",
        &["codeterm", "update", "check"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "config",
        &["codeterm", "config", "get", "some.key"],
        Local::PreSocket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "bridge",
        &["codeterm", "bridge", "restart"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "mesh",
        &["codeterm", "mesh", "status"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "mem",
        &["codeterm", "mem", "search", "q"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "plan",
        &["codeterm", "plan", "list"],
        Local::Socket,
        MeshRelay::SocketRpc,
        ApiVerdict::Unsupported("plan management is local-only — run it on the target machine"),
    ),
    (
        "spec",
        &["codeterm", "spec", "list"],
        Local::Socket,
        MeshRelay::SocketRpc,
        ApiVerdict::Unsupported("spec management is local-only — run it on the target machine"),
    ),
    (
        "effort",
        &["codeterm", "effort", "show", "pl_1"],
        Local::Socket,
        MeshRelay::SocketRpc,
        ApiVerdict::Unsupported("effort management is local-only — run it on the target machine"),
    ),
    (
        "task",
        &["codeterm", "task", "list"],
        Local::Socket,
        MeshRelay::SocketRpc,
        ApiVerdict::Unsupported(TASK_LOCAL_ONLY),
    ),
    (
        "inbox",
        &["codeterm", "inbox", "add", "--name", "n"],
        Local::Socket,
        MeshRelay::SocketRpc,
        ApiVerdict::Unsupported(TASK_LOCAL_ONLY),
    ),
    (
        "canvas",
        &["codeterm", "canvas", "filter", "--facet", "area:layout"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Unsupported("canvas control is local-only — run it on the target machine"),
    ),
    (
        "sessions",
        &["codeterm", "sessions", "search", "q"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "usage",
        &["codeterm", "usage", "latest"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "system",
        &["codeterm", "system", "autostart-status"],
        Local::PreSocket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "docs",
        &["codeterm", "docs"],
        Local::PreSocket,
        MeshRelay::Fallthrough,
        ApiVerdict::Rest,
    ),
    (
        "debug",
        &["codeterm", "debug", "crash", "--kind", "panic"],
        Local::Socket,
        MeshRelay::Fallthrough,
        ApiVerdict::RefusedInPlace,
    ),
];

/// `mem` splits below the variant, so its pre-socket rows are frozen per
/// sub-action rather than per command.
const FROZEN_MEM_LOCAL: &[(&[&str], Local)] = &[
    (&["codeterm", "mem", "search", "q"], Local::Socket),
    (
        &["codeterm", "mem", "run-worker", "dreaming-light"],
        Local::PreSocket,
    ),
    (&["codeterm", "mem", "dream", "run"], Local::PreSocket),
    (
        &[
            "codeterm", "mem", "graph", "create", "--id", "e1", "--type", "Person",
        ],
        Local::PreSocketOrSocket,
    ),
];

fn parse(argv: &[&str]) -> Commands {
    Cli::try_parse_from(argv)
        .unwrap_or_else(|error| panic!("{argv:?} must parse: {error}"))
        .command
}

#[test]
fn every_command_declares_transport_exactly_once() {
    let parser_names: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .filter(|name| name != "help")
        .collect();

    for name in &parser_names {
        assert_eq!(
            FROZEN.iter().filter(|row| row.0 == name.as_str()).count(),
            1,
            "`{name}` must have exactly one frozen transport row"
        );
    }
    for (name, ..) in FROZEN {
        assert!(
            parser_names.iter().any(|parsed| parsed.as_str() == *name),
            "frozen row `{name}` is not a command the parser exposes"
        );
    }
    assert_eq!(
        FROZEN.len(),
        parser_names.len(),
        "frozen table and parser must cover the same command set"
    );
}

#[test]
fn transport_table_matches_pre_change_behaviour() {
    for (name, argv, local, mesh_relay, api_verdict) in FROZEN {
        let declared = transport_for(&parse(argv));
        assert_eq!(declared.local, *local, "`{name}` local handling drifted");
        assert_eq!(
            declared.mesh_relay, *mesh_relay,
            "`{name}` mesh interception drifted"
        );
        assert_eq!(
            declared.api_verdict, *api_verdict,
            "`{name}` api_map verdict drifted"
        );
    }

    for (argv, local) in FROZEN_MEM_LOCAL {
        assert_eq!(
            transport_for(&parse(argv)).local,
            *local,
            "{argv:?} local handling drifted"
        );
    }
}

#[test]
fn mesh_unsupported_reasons_are_preserved() {
    let mut checked = 0;
    for (name, argv, _, _, api_verdict) in FROZEN {
        let ApiVerdict::Unsupported(reason) = api_verdict else {
            continue;
        };
        let (method, emitted, body) = cli_command_to_api(&parse(argv));
        assert_eq!(method, UNSUPPORTED_OVER_MESH, "`{name}` lost its refusal");
        assert_eq!(emitted.as_str(), *reason, "`{name}` reason was reworded");
        assert!(body.is_none(), "`{name}` refusal must carry no body");
        checked += 1;
    }
    assert_eq!(checked, 13, "unsupported command count changed");
}

#[test]
fn api_map_still_builds_identical_rest_bodies() {
    let cases: &[(&[&str], &str, &str)] = &[
        (&["codeterm", "identify"], "GET", "/api/identify"),
        (&["codeterm", "workspace", "list"], "GET", "/api/workspaces"),
        (
            &["codeterm", "cron", "delete", "cron_1"],
            "DELETE",
            "/api/crons/cron_1",
        ),
        (
            &["codeterm", "usage", "latest", "--provider", "claude"],
            "GET",
            "/api/usage/latest?provider=claude",
        ),
        (
            &["codeterm", "system", "autostart-status"],
            "GET",
            "/api/identify",
        ),
        (&["codeterm", "docs"], "GET", "/api/identify"),
    ];

    for (argv, method, path) in cases {
        let (got_method, got_path, got_body) = cli_command_to_api(&parse(argv));
        assert_eq!(got_method.as_str(), *method, "{argv:?} method drifted");
        assert_eq!(got_path.as_str(), *path, "{argv:?} path drifted");
        assert!(got_body.is_none(), "{argv:?} gained a body");
    }

    // Body shape is asserted through parsed JSON so key order is not the test.
    let (method, path, body) = cli_command_to_api(&parse(&[
        "codeterm", "notify", "send", "hello", "--level", "info",
    ]));
    assert_eq!(method.as_str(), "POST");
    assert_eq!(path.as_str(), "/api/notifications");
    let body: serde_json::Value =
        serde_json::from_str(&body.expect("notify send must carry a body")).unwrap();
    assert_eq!(body["title"], serde_json::json!("hello"));
    assert_eq!(body["level"], serde_json::json!("info"));
    assert_eq!(body["body"], serde_json::Value::Null);
    assert_eq!(
        body.as_object().map(|map| map.len()),
        Some(3),
        "notify send body gained or lost a field"
    );

    let (method, path, body) = cli_command_to_api(&parse(&[
        "codeterm",
        "team",
        "pair",
        "--orchestrator",
        "claude:opus",
        "--planner",
        "codex",
    ]));
    assert_eq!(method.as_str(), "POST");
    assert_eq!(path.as_str(), "/api/orchestration/new");
    let body: serde_json::Value =
        serde_json::from_str(&body.expect("team pair must carry a body")).unwrap();
    assert_eq!(body["orchestrator_agent"], serde_json::json!("claude"));
    assert_eq!(body["orchestrator_model"], serde_json::json!("opus"));
    assert_eq!(body["planner_agent"], serde_json::json!("codex"));
    assert!(body.get("planner_model").is_none());
}

#[test]
fn no_wildcard_arm_in_transport_match() {
    let source = include_str!("../transport.rs");
    let start = source
        .find("pub(crate) fn transport_for")
        .expect("transport_for must exist");
    let body = &source[start..];
    let end = body.find("\n}\n").expect("transport_for must terminate");
    let body = &body[..end];

    assert!(
        !body.contains("_ =>"),
        "transport_for must stay wildcard-free so a new variant fails to compile"
    );
    assert!(
        !body.contains("_ if"),
        "transport_for must not smuggle a default through a guard"
    );
    assert!(
        body.contains("Commands::Debug"),
        "the slice under test must be the whole transport_for match"
    );
}

#[test]
fn team_pair_exception_is_declared() {
    let pair = parse(&[
        "codeterm",
        "team",
        "pair",
        "--orchestrator",
        "claude",
        "--planner",
        "claude",
    ]);
    let declared = transport_for(&pair);
    assert_eq!(
        declared.mesh_relay,
        MeshRelay::Fallthrough,
        "team pair must reach the REST mapping"
    );
    assert_eq!(declared.api_verdict, ApiVerdict::Rest);
    assert!(!crate::mesh_cli::is_socket_relayed(&pair));

    let (method, path, _) = cli_command_to_api(&pair);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/orchestration/new");

    for argv in [
        vec!["codeterm", "team", "list"],
        vec!["codeterm", "team", "show", "team_1"],
        vec!["codeterm", "team", "rename", "team_1", "name"],
        vec!["codeterm", "team", "close", "team_1"],
        vec!["codeterm", "team", "adopt-plan", "team_1", "pl_1"],
    ] {
        let command = parse(&argv);
        let declared = transport_for(&command);
        assert_eq!(
            declared.mesh_relay,
            MeshRelay::SocketRpc,
            "{argv:?} must keep the MeshRpcClient path"
        );
        assert_eq!(
            declared.api_verdict,
            ApiVerdict::Unsupported(TEAM_LOCAL_ONLY),
            "{argv:?} must keep the unreachable verdict recorded"
        );
        assert!(crate::mesh_cli::is_socket_relayed(&command));
    }
}
