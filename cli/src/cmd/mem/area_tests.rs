use clap::Parser;

use super::socket::socket_request;
use crate::{Cli, Commands};

#[test]
fn mem_area_merge_parses_and_routes_dry_run() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "mem",
        "area",
        "merge",
        "dev",
        "--into",
        "development",
        "--dry-run",
    ])
    .unwrap();
    let Commands::Mem { action } = cli.command else {
        panic!("expected mem")
    };
    let (method, payload) = socket_request(&action).unwrap();
    assert_eq!(method, "mem.area_merge");
    let payload = payload.unwrap();
    assert_eq!(payload["from"], "dev");
    assert_eq!(payload["into"], "development");
    assert_eq!(payload["dry_run"], true);
    let (method, path, body) =
        crate::mesh_cli::api_map::cli_command_to_api(&Commands::Mem { action });
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/mem/area/merge");
    let body: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
    assert_eq!(body["from"], "dev");
    assert_eq!(body["into"], "development");
    assert_eq!(body["dry_run"], true);
}

#[test]
fn mem_area_merge_requires_source_and_destination() {
    for args in [
        vec!["codeterm", "mem", "area", "merge"],
        vec!["codeterm", "mem", "area", "merge", "dev"],
        vec!["codeterm", "mem", "area", "merge", "--into", "development"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[test]
fn mem_area_merge_without_dry_run_requests_apply() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "mem",
        "area",
        "merge",
        "dev",
        "--into",
        "development",
    ])
    .unwrap();
    let Commands::Mem { action } = cli.command else {
        panic!("expected mem")
    };
    assert_eq!(
        socket_request(&action).unwrap().1.unwrap()["dry_run"],
        false
    );
}

#[test]
fn mem_area_merge_renders_per_kind_counts() {
    let response = serde_json::json!({ "from": "dev", "into": "development", "dry_run": true, "counts": { "fact": 2, "note": 1 } });
    let output = super::render_area_merge(&response).unwrap();
    assert!(output.contains("fact: 2"));
    assert!(output.contains("note: 1"));
    assert!(super::render_area_merge(&serde_json::json!({ "stats": 3 })).is_none());
}
