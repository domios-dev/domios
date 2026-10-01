use clap::Parser;

use super::socket::socket_request;
use crate::{Cli, Commands, MemAction, MemCalibrationAction};

fn mem_action(args: &[&str]) -> MemAction {
    match Cli::try_parse_from(args).unwrap().command {
        Commands::Mem { action } => action,
        _ => panic!("expected a mem command for {args:?}"),
    }
}

#[test]
fn mem_merge_parses_keeper_lesser_body_and_conflict() {
    let action = mem_action(&[
        "codeterm",
        "mem",
        "merge",
        "--keeper",
        "fact_keep",
        "--lesser",
        "fact_dup",
        "--body",
        "merged text",
        "--conflict",
    ]);
    let MemAction::Merge {
        keeper,
        lesser,
        body,
        body_file,
        conflict,
        changed_by,
    } = action
    else {
        panic!("expected Merge");
    };
    assert_eq!(keeper, "fact_keep");
    assert_eq!(lesser, "fact_dup");
    assert_eq!(body.as_deref(), Some("merged text"));
    assert!(body_file.is_none());
    assert!(conflict);
    assert!(changed_by.is_none());
}

#[test]
fn mem_merge_requires_keeper_and_lesser() {
    assert!(Cli::try_parse_from(["codeterm", "mem", "merge", "--keeper", "a"]).is_err());
    assert!(Cli::try_parse_from(["codeterm", "mem", "merge", "--lesser", "b"]).is_err());
}

#[test]
fn mem_merge_socket_request_carries_the_merge_shape() {
    let action = mem_action(&[
        "codeterm", "mem", "merge", "--keeper", "k", "--lesser", "l", "--body", "b",
    ]);
    let (method, params) = socket_request(&action).expect("merge is socket-routed");
    assert_eq!(method, "mem.merge");
    let params = params.unwrap();
    assert_eq!(params["keeper"], "k");
    assert_eq!(params["lesser"], "l");
    assert_eq!(params["body"], "b");
    assert_eq!(params["conflict"], false);
    assert!(params.get("worker_id").is_some());
}

#[test]
fn mem_merge_body_file_is_read_into_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged.md");
    std::fs::write(&path, "from file").unwrap();
    let action = mem_action(&[
        "codeterm",
        "mem",
        "merge",
        "--keeper",
        "k",
        "--lesser",
        "l",
        "--body-file",
        path.to_str().unwrap(),
    ]);
    let (_, params) = socket_request(&action).unwrap();
    assert_eq!(params.unwrap()["body"], "from file");
}

#[test]
fn mem_merge_mesh_relay_posts_to_the_merge_route() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "mem",
        "merge",
        "--keeper",
        "k",
        "--lesser",
        "l",
        "--conflict",
    ])
    .unwrap();
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&cli.command);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/mem/merge");
    let body: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
    assert_eq!(body["keeper"], "k");
    assert_eq!(body["lesser"], "l");
    assert_eq!(body["conflict"], true);
}

#[test]
fn mem_merge_calibration_stale_dry_run_parses() {
    let action = mem_action(&[
        "codeterm",
        "mem",
        "calibration",
        "stale",
        "--dry-run",
        "--limit",
        "50",
    ]);
    let MemAction::Calibration {
        action: MemCalibrationAction::Stale { dry_run, limit },
    } = action
    else {
        panic!("expected calibration stale");
    };
    assert!(dry_run);
    assert_eq!(limit, Some(50));

    let cli =
        Cli::try_parse_from(["codeterm", "mem", "calibration", "stale", "--dry-run"]).unwrap();
    let (_, path, _) = crate::mesh_cli::api_map::cli_command_to_api(&cli.command);
    assert_eq!(path, "/api/mem/calibration/stale?dry_run=true");
}
