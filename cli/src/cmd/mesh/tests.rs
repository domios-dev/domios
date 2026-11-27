use super::parse_mesh_response;

#[test]
fn mesh_listener_refusal_is_an_error_response() {
    let result = parse_mesh_response(
        reqwest::StatusCode::CONFLICT,
        "mesh_listener_not_remotely_ready",
    );

    let error = result.expect_err("an unavailable mesh listener must fail the CLI command");
    assert!(error.contains("HTTP 409"));
    assert!(error.contains("mesh_listener_not_remotely_ready"));
}

#[test]
fn mesh_pairing_verbs_are_removed() {
    use clap::Parser;
    for removed in [
        "link",
        "disconnect",
        "unrevoke",
        "connect",
        "approve",
        "reject",
        "discover",
        "broadcast",
        "stop-broadcast",
    ] {
        assert!(
            crate::Cli::try_parse_from(["domios", "mesh", removed, "installation"]).is_err(),
            "{removed}"
        );
    }
}

#[test]
fn mesh_status_exposes_publication_refusal_and_retry_deadline() {
    let state = serde_json::json!({
        "publication": "published",
        "publication_failure": {"reason": "rate-limited", "retry_after_seconds": 7},
        "retry_at": 107
    });
    let lines = super::account_mesh_status_lines(&state).join("\n");
    assert!(lines.contains("published"));
    assert!(lines.contains("rate-limited"));
    assert!(lines.contains("107"));
    assert!(lines.contains("waiting to retry"));
}
#[test]
fn mesh_status_explains_required_reauthentication() {
    let lines = super::account_mesh_status_lines(&serde_json::json!({
        "enrollment": "refused",
        "last_refusal": { "reason": "reauth_required" },
        "retry_at": null
    }));
    assert_eq!(lines, ["Sign in again to add this machine to your mesh"]);
}

#[test]
fn mesh_status_says_plainly_when_the_build_cannot_join() {
    let lines = super::account_mesh_status_lines(&serde_json::json!({
        "publication": "none",
        "last_refusal": { "reason": "proof_invalid" },
    }));
    assert!(lines[0].starts_with("This build cannot join this account's mesh"));
    assert!(lines.iter().any(|line| line.contains("proof_invalid")));
}

#[test]
fn mesh_account_presence_is_distinct_from_offline_and_expired() {
    use serde_json::json;
    let mut row =
        json!({"active":true,"presence":"signed_out","membership_expires_at":1,"endpoint":null});
    assert_eq!(
        super::account_machine_status(&row, 10),
        "Signed out (actions disabled)"
    );
    row["presence"] = json!("present");
    assert_eq!(
        super::account_machine_status(&row, 10),
        "Membership expired"
    );
    row["membership_expires_at"] = json!(20);
    assert_eq!(super::account_machine_status(&row, 10), "Offline");
    row["endpoint"] = json!({"expires_at":20});
    assert_eq!(super::account_machine_status(&row, 10), "Present");
    row["active"] = json!(false);
    assert_eq!(super::account_machine_status(&row, 10), "Removed");
    let state = json!({"registry":{"rows":[
        {"display_name":"Laptop","active":true,"presence":"signed_out"},
        {"display_name":"Old","active":false,"eligibility":"revoked","presence":"present"}
    ]}});
    let status_lines = super::account_mesh_status_lines(&state);
    assert!(status_lines.contains(&"  Laptop: Signed out (actions disabled)".to_owned()));
    assert!(!status_lines.iter().any(|line| line.contains("Old")));
}

#[test]
fn mesh_remove_parses_machine_target_and_blocks_mesh_relay() {
    use clap::Parser;
    let parsed = crate::Cli::try_parse_from(["domios", "mesh", "remove", "BLACKSTAR #4"])
        .expect("mesh remove parses");
    match &parsed.command {
        crate::Commands::Mesh {
            action:
                crate::MeshAction::Remove {
                    machine,
                    stale,
                    older_than,
                    dry_run,
                },
        } => {
            assert_eq!(machine.as_deref(), Some("BLACKSTAR #4"));
            assert!(!stale);
            assert_eq!(*older_than, None);
            assert!(!dry_run);
        }
        _ => panic!("expected MeshAction::Remove"),
    }
    let (method, _, _) = crate::mesh_cli::api_map::cli_command_to_api(&parsed.command);
    assert_eq!(method, crate::mesh_cli::api_map::UNSUPPORTED_OVER_MESH);
}

#[test]
fn mesh_remove_stale_parses_flags_and_enforces_conflicts() {
    use clap::Parser;
    let parsed = crate::Cli::try_parse_from([
        "domios",
        "mesh",
        "remove",
        "--stale",
        "--older-than",
        "14",
        "--dry-run",
    ])
    .expect("mesh remove --stale parses");
    match &parsed.command {
        crate::Commands::Mesh {
            action:
                crate::MeshAction::Remove {
                    machine,
                    stale,
                    older_than,
                    dry_run,
                },
        } => {
            assert_eq!(*machine, None);
            assert!(*stale);
            assert_eq!(*older_than, Some(14));
            assert!(*dry_run);
        }
        _ => panic!("expected MeshAction::Remove --stale"),
    }
    assert!(crate::Cli::try_parse_from(["domios", "mesh", "remove"]).is_err());
    assert!(crate::Cli::try_parse_from(["domios", "mesh", "remove", "--dry-run"]).is_err());
    assert!(crate::Cli::try_parse_from(["domios", "mesh", "remove", "m1", "--stale"]).is_err());
    assert!(crate::Cli::try_parse_from(["domios", "mesh", "remove", "m1", "--dry-run"]).is_err());
    assert!(
        crate::Cli::try_parse_from(["domios", "mesh", "remove", "m1", "--older-than", "7"])
            .is_err()
    );
}

#[test]
fn mesh_remove_stale_formats_dry_run_and_batch_results() {
    use serde_json::json;
    let empty = json!({
        "dry_run": true,
        "matched": 0,
        "removed": 0,
        "already_removed": 0,
        "skipped": 0,
        "rows": []
    });
    assert_eq!(
        super::format_remove_stale_lines(&empty),
        vec!["No stale machines found."]
    );

    let dry_run = json!({
        "dry_run": true,
        "matched": 2,
        "removed": 0,
        "already_removed": 0,
        "skipped": 0,
        "rows": [
            {
                "installation_id": "inst-1",
                "display_name": "Old Laptop",
                "reason": "membership_expired",
                "status": "would_remove",
                "slot_revision": 1
            },
            {
                "installation_id": "inst-2",
                "display_name": "Dormant Box",
                "reason": "signed_out",
                "status": "would_remove",
                "slot_revision": 2
            }
        ]
    });
    assert_eq!(
        super::format_remove_stale_lines(&dry_run),
        vec![
            "Dry run: 2 stale machines would be removed:",
            "  Old Laptop (inst-1): would remove (membership expired)",
            "  Dormant Box (inst-2): would remove (signed out)",
        ]
    );

    let applied = json!({
        "dry_run": false,
        "matched": 3,
        "removed": 1,
        "already_removed": 1,
        "skipped": 1,
        "rows": [
            {
                "installation_id": "inst-1",
                "display_name": "Old Laptop",
                "reason": "membership_expired",
                "status": "removed",
                "slot_revision": 2
            },
            {
                "installation_id": "inst-2",
                "display_name": "Gone Box",
                "reason": "signed_out",
                "status": "already_removed",
                "slot_revision": 2
            },
            {
                "installation_id": "inst-3",
                "display_name": "Live Box",
                "reason": null,
                "status": "refused_online",
                "slot_revision": 1
            }
        ]
    });
    assert_eq!(
        super::format_remove_stale_lines(&applied),
        vec![
            "Removed 1 stale machine (1 already removed, 1 skipped):",
            "  Old Laptop (inst-1): removed (membership expired)",
            "  Gone Box (inst-2): already removed (signed out)",
            "  Live Box (inst-3): skipped (online)",
        ]
    );
}

#[test]
fn mesh_remove_stale_excludes_live_cloud_machines_and_explains_exclusion_on_shared_fixture() {
    use clap::Parser;
    use serde_json::json;

    let parsed = crate::Cli::try_parse_from(["domios", "mesh", "remove", "--stale", "--dry-run"])
        .expect("mesh remove --stale --dry-run parses");
    match &parsed.command {
        crate::Commands::Mesh {
            action:
                crate::MeshAction::Remove {
                    stale: true,
                    older_than,
                    dry_run: true,
                    ..
                },
        } => {
            assert_eq!(
                super::build_remove_stale_body(*older_than, true),
                json!({ "dry_run": true })
            );
        }
        _ => panic!("expected MeshAction::Remove --stale --dry-run"),
    }

    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/mesh-stale-cleanup.json"
    )))
    .expect("parse shared stale cleanup fixture");

    let expected_selected = fixture["expected_selected"]
        .as_array()
        .expect("expected_selected array");
    let expected_excluded = fixture["expected_excluded_cloud_ids"]
        .as_array()
        .expect("expected_excluded_cloud_ids array");
    assert!(!expected_selected.is_empty());
    assert!(!expected_excluded.is_empty());

    let dry_run_rows: Vec<serde_json::Value> = expected_selected
        .iter()
        .map(|entry| {
            json!({
                "installation_id": entry["installation_id"],
                "display_name": entry["display_name"],
                "reason": entry["reason"],
                "status": "would_remove",
                "slot_revision": 1
            })
        })
        .collect();
    let dry_run_payload = json!({
        "dry_run": true,
        "matched": dry_run_rows.len(),
        "removed": 0,
        "already_removed": 0,
        "skipped": 0,
        "excluded_cloud_count": expected_excluded.len(),
        "rows": dry_run_rows
    });
    let dry_run_lines = super::format_remove_stale_lines(&dry_run_payload);
    assert_eq!(
        dry_run_lines.first().map(String::as_str),
        Some("Dry run: 4 stale machines would be removed:")
    );
    for entry in expected_selected {
        let id = entry["installation_id"].as_str().unwrap();
        let name = entry["display_name"].as_str().unwrap();
        assert!(
            dry_run_lines
                .iter()
                .any(|line| line.contains(id) && line.contains(name)),
            "missing expected stale candidate {name} ({id}) in {dry_run_lines:?}"
        );
    }
    for excluded_id in expected_excluded {
        let id = excluded_id.as_str().unwrap();
        assert!(
            !dry_run_lines.iter().any(|line| line.contains(id)),
            "live cloud machine {id} must never appear in stale removal output: {dry_run_lines:?}"
        );
    }
    assert_eq!(
        dry_run_lines.last().map(String::as_str),
        Some(super::CLOUD_EXCLUSION_NOTE)
    );

    // When only live cloud machines look stale, report none matched and explain cloud exclusion.
    let only_cloud_excluded = json!({
        "dry_run": false,
        "matched": 0,
        "removed": 0,
        "already_removed": 0,
        "skipped": 0,
        "excluded_cloud_count": expected_excluded.len(),
        "rows": []
    });
    assert_eq!(
        super::format_remove_stale_lines(&only_cloud_excluded),
        vec![
            "No stale machines found.".to_owned(),
            super::CLOUD_EXCLUSION_NOTE.to_owned(),
        ]
    );

    // Explicit refusal of a cloud machine row prints a clear skip reason and the exclusion note.
    let refused_cloud = json!({
        "dry_run": false,
        "matched": 1,
        "removed": 0,
        "already_removed": 0,
        "skipped": 1,
        "excluded_cloud_count": 1,
        "rows": [
            {
                "installation_id": "880cbbeedc6f433f8f332c9af20bc8c8",
                "display_name": "quill-fly-124-warm",
                "reason": null,
                "status": "refused_cloud",
                "slot_revision": 2
            }
        ]
    });
    assert_eq!(
        super::format_remove_stale_lines(&refused_cloud),
        vec![
            "Removed 0 stale machines (1 skipped):".to_owned(),
            "  quill-fly-124-warm (880cbbeedc6f433f8f332c9af20bc8c8): skipped (cloud machine)"
                .to_owned(),
            super::CLOUD_EXCLUSION_NOTE.to_owned(),
        ]
    );
}

#[test]
fn mesh_identity_set_accepts_kind_only_and_rejects_color() {
    use clap::Parser;
    let parsed =
        crate::Cli::try_parse_from(["domios", "mesh", "identity", "set", "--kind", "laptop"])
            .expect("mesh identity set --kind parses");
    let (method, path, body) = crate::mesh_cli::api_map::cli_command_to_api(&parsed.command);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/mesh/identity");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(body.as_deref().unwrap()).unwrap(),
        serde_json::json!({ "kind": "laptop", "color": "graphite" })
    );

    crate::Cli::try_parse_from([
        "domios", "mesh", "identity", "set", "--kind", "server", "--peer", "ubunta",
    ])
    .expect("mesh identity set --peer parses");

    assert!(crate::Cli::try_parse_from([
        "domios", "mesh", "identity", "set", "--kind", "laptop", "--color", "teal",
    ])
    .is_err());
}

#[test]
fn mesh_status_agrees_with_live_connection_when_cloud_endpoint_is_stale() {
    use serde_json::json;
    let state = json!({
        "local_installation_id": "inst-local",
        "publication": "published",
        "membership_expires_at": u64::MAX,
        "pairs": [
            {
                "instance_id": "66c926f1-peer",
                "installation_id": "7a835b57-r161",
                "online": true,
                "name": "acceptance-r161"
            }
        ],
        "registry": {
            "rows": [
                {
                    "installation_id": "inst-local",
                    "display_name": "w1221",
                    "active": true,
                    "presence": "present",
                    "membership_expires_at": 1,
                    "endpoint": null
                },
                {
                    "installation_id": "7a835b57-r161",
                    "display_name": "acceptance-r161",
                    "active": true,
                    "presence": "present",
                    "membership_expires_at": u64::MAX,
                    "endpoint": { "expires_at": 1 }
                },
                {
                    "installation_id": "inst-offline",
                    "display_name": "dormant",
                    "active": true,
                    "presence": "present",
                    "membership_expires_at": u64::MAX,
                    "endpoint": null
                }
            ]
        }
    });
    let lines = super::account_mesh_status_lines(&state);
    assert!(
        lines.contains(&"  acceptance-r161: Connected".to_owned()),
        "live connected peer must not appear Offline: {lines:?}"
    );
    assert!(
        lines.contains(&"  w1221: This machine".to_owned()),
        "published local machine must not appear Offline: {lines:?}"
    );
    assert!(
        lines.contains(&"  dormant: Offline".to_owned()),
        "unconnected machine without fresh endpoint remains Offline: {lines:?}"
    );
}
