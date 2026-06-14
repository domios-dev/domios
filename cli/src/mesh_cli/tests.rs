use super::api_map::cli_command_to_api;
use super::parse_ip_port;
use super::{MeshAction, MeshPeerAction};
use crate::{Commands, UpdateAction};

fn update_cmd(action: Option<UpdateAction>, manifest_url: Option<String>) -> Commands {
    Commands::Update {
        action,
        manifest_url,
        after_update: None,
    }
}

#[test]
fn check_maps_to_get_check() {
    let (m, p, b) = cli_command_to_api(&update_cmd(None, None));
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/update/check");
    assert!(b.is_none());
}

#[test]
fn check_force_maps_to_query_param() {
    let (m, p, _) =
        cli_command_to_api(&update_cmd(Some(UpdateAction::Check { force: true }), None));
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/update/check?force=true");
}

#[test]
fn apply_maps_to_install_auto() {
    let (m, p, b) = cli_command_to_api(&update_cmd(
        Some(UpdateAction::Install {
            auto: true,
            manifest_url: None,
            from_file: None,
            continue_prompt: None,
            continue_pane: None,
            after_update: None,
        }),
        None,
    ));
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/update/install?method=auto");
    assert!(b.is_none());
}

#[test]
fn apply_with_manifest_url_appends_query() {
    let (m, p, _) = cli_command_to_api(&update_cmd(
        Some(UpdateAction::Install {
            auto: true,
            manifest_url: None,
            from_file: None,
            continue_prompt: None,
            continue_pane: None,
            after_update: None,
        }),
        Some("http://localhost:9999/latest.json".to_string()),
    ));
    assert_eq!(m, "POST");
    assert!(p.starts_with("/api/update/install?method=auto&manifest_url="));
    assert!(p.contains("localhost%3A9999"));
}

#[test]
fn update_install_local_only_options_are_rejected_over_mesh() {
    let (m, reason, b) = cli_command_to_api(&update_cmd(
        Some(UpdateAction::Install {
            auto: true,
            manifest_url: None,
            from_file: Some("C:/tmp/CodeTerm.exe".into()),
            continue_prompt: Some("verify".into()),
            continue_pane: Some("pane_1".into()),
            after_update: None,
        }),
        None,
    ));
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("local-only"), "reason: {reason}");
    assert!(b.is_none());
}

#[test]
fn capabilities_is_not_misrouted_to_identify() {
    let (m, reason, b) = cli_command_to_api(&Commands::Capabilities);
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("socket-only"), "reason: {reason}");
    assert!(b.is_none());
}

#[test]
fn mesh_refresh_maps_to_post_refresh() {
    let (m, p, b) = cli_command_to_api(&Commands::Mesh {
        action: MeshAction::Refresh,
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/refresh");
    assert!(b.is_none());
}

#[test]
fn mesh_announce_no_target_maps_to_announce_all() {
    let (m, p, b) = cli_command_to_api(&Commands::Mesh {
        action: MeshAction::Announce { to: None },
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/announce/all");
    assert!(b.is_none());
}

#[test]
fn mesh_announce_to_target_appends_query_param() {
    let (m, p, _) = cli_command_to_api(&Commands::Mesh {
        action: MeshAction::Announce {
            to: Some("BLACKSTAR".into()),
        },
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/announce/all?to=BLACKSTAR");
}

#[test]
fn parse_ip_port_plain_ipv4() {
    assert_eq!(
        parse_ip_port("192.168.1.1"),
        ("192.168.1.1".to_string(), None)
    );
}

#[test]
fn parse_ip_port_ipv4_with_port() {
    assert_eq!(
        parse_ip_port("100.84.1.2:7685"),
        ("100.84.1.2".to_string(), Some(7685))
    );
}

#[test]
fn parse_ip_port_ipv6_bracketed_with_port() {
    let (ip, port) = parse_ip_port("[fe80::1]:7685");
    assert_eq!(ip, "fe80::1");
    assert_eq!(port, Some(7685));
}

#[test]
fn parse_ip_port_bare_ipv6_no_port() {
    let (ip, port) = parse_ip_port("fe80::1");
    assert_eq!(ip, "fe80::1");
    assert_eq!(port, None);
}

#[test]
fn mesh_peer_set_addr_serializes_body_with_port() {
    let (m, p, b) = cli_command_to_api(&Commands::Mesh {
        action: MeshAction::Peer {
            action: MeshPeerAction::SetAddr {
                peer: "BLACKSTAR".into(),
                addr: "100.64.10.1:7685".into(),
            },
        },
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/peers/BLACKSTAR/addr");
    let body = b.expect("body");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ip"], "100.64.10.1");
    assert_eq!(v["port"], 7685);
}

#[test]
fn mesh_peer_set_addr_omits_port_when_absent() {
    let (_, _, b) = cli_command_to_api(&Commands::Mesh {
        action: MeshAction::Peer {
            action: MeshPeerAction::SetAddr {
                peer: "X".into(),
                addr: "10.0.0.1".into(),
            },
        },
    });
    let v: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(v["ip"], "10.0.0.1");
    assert!(v.get("port").is_none());
}

#[test]
fn mem_search_relays_to_search_endpoint_with_query() {
    use crate::MemAction;
    let cmd = Commands::Mem {
        action: MemAction::Search {
            query: "windows release build".into(),
            kind: Some("fact".into()),
            area: None,
            scope: None,
            limit: 5,
            rerank: false,
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert!(p.starts_with("/api/mem/search?"), "got: {}", p);
    assert!(
        p.contains("q=windows%20release%20build"),
        "query missing/unencoded: {}",
        p
    );
    assert!(p.contains("kind=fact"), "kind missing: {}", p);
    assert!(p.contains("limit=5"), "limit missing: {}", p);
    assert!(b.is_none());
}

#[test]
fn mem_save_relays_to_items_post() {
    use crate::MemAction;
    let cmd = Commands::Mem {
        action: MemAction::Save {
            kind: "fact".into(),
            title: Some("test".into()),
            body: Some("body content".into()),
            summary: None,
            tags: None,
            area: Some("release".into()),
            importance: 4.0,
            scope: None,
            workspace: None,
            pin: false,
            source_path: None,
            replace: false,
            skip_extract: false,
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mem/items");
    let body: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(body["kind"], "fact");
    assert_eq!(body["title"], "test");
    assert_eq!(body["body"], "body content");
    assert_eq!(body["area"], "release");
    assert_eq!(body["importance"], 4.0);
}

#[test]
fn mem_graph_create_relays_with_server_key_names() {
    use crate::{MemAction, MemGraphAction};
    let cmd = Commands::Mem {
        action: MemAction::Graph {
            action: MemGraphAction::Create {
                id: "person_kai".into(),
                entity_type: "Person".into(),
                props: r#"{"name":"Kai"}"#.into(),
                scope: None,
            },
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mem/graph/entities");
    let body: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(body["id"], "person_kai");
    // Use server JSON keys (`type`, `properties`) here.
    // Reject local aliases like `entity_type`/`props`.
    assert_eq!(body["type"], "Person");
    assert_eq!(body["properties"]["name"], "Kai");
    assert!(
        body.get("entity_type").is_none(),
        "stale key leaked: {}",
        body
    );
    assert!(body.get("props").is_none(), "stale key leaked: {}", body);
}

#[test]
fn agent_providers_relays_to_providers_endpoint() {
    use crate::AgentAction;
    let (m, p, b) = cli_command_to_api(&Commands::Agent {
        action: AgentAction::Providers,
    });
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/agents/providers");
    assert!(b.is_none());
}

#[test]
fn agent_models_relays_to_providers_endpoint() {
    use crate::AgentAction;
    let cmd = Commands::Agent {
        action: AgentAction::Models {
            provider: "claude".into(),
        },
    };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/agents/providers");
}

#[test]
fn mesh_cron_create_forwards_all_advanced_fields() {
    use crate::CronAction;
    // Keep advanced cron flags on both mesh and local paths.
    let cmd = Commands::Cron {
        action: CronAction::Create {
            name: "Nightly".into(),
            schedule: "0 3 * * *".into(),
            task: "do x".into(),
            workspace: Some("ws_1".into()),
            agent: "claude".into(),
            model: None,
            cwd: None,
            description: None,
            creator_pane: Some("pane_9".into()),
            once: true,
            jitter_max_ms: Some(5000),
            tz: Some("Europe/Riga".into()),
            timeout_secs: Some(1800),
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/crons");
    let body: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(body["workspace_id"], "ws_1");
    assert_eq!(body["oneshot"], true);
    assert_eq!(body["jitter_max_ms"], 5000);
    assert_eq!(body["tz"], "Europe/Riga");
    assert_eq!(body["timeout_seconds"], 1800);
    assert_eq!(body["creator_pane_id"], "pane_9");
}

#[test]
fn pane_new_relays_cmd_field() {
    use crate::PaneAction;
    let cmd = Commands::Pane {
        action: PaneAction::New {
            workspace: Some("ws_1".into()),
            cwd: Some("C:/Developer/codeterm".into()),
            title: Some("runner".into()),
            cmd: Some("pnpm test".into()),
            eager: true,
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/panes");
    let body: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(body["workspace_id"], "ws_1");
    assert_eq!(body["cwd"], "C:/Developer/codeterm");
    assert_eq!(body["title"], "runner");
    assert_eq!(body["cmd"], "pnpm test");
    assert_eq!(body["eager"], true);
}

#[test]
fn mesh_local_only_mem_subcommands_are_rejected_not_misrouted() {
    use crate::MemAction;
    // Mesh must return errors for local-only mem commands, not fake /api/mem/stats.
    let cmd = Commands::Mem {
        action: MemAction::RunWorker {
            kind: "dreaming-light".into(),
            db_path: None,
        },
    };
    let (m, reason, b) = cli_command_to_api(&cmd);
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(
        reason.contains("local-only"),
        "reason should explain why: {reason}"
    );
    assert!(b.is_none());
}

#[test]
fn mem_dream_apply_is_rejected_not_misrouted_to_stats() {
    use crate::{MemAction, MemDreamAction};
    let cmd = Commands::Mem {
        action: MemAction::Dream {
            action: MemDreamAction::Apply {
                plan_file: "plan.md".into(),
                db_path: None,
            },
        },
    };
    let (m, reason, b) = cli_command_to_api(&cmd);
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("local-only"), "reason: {reason}");
    assert!(b.is_none());
}

#[test]
fn mem_graph_local_only_reads_are_rejected_not_misrouted_to_stats() {
    use crate::{MemAction, MemGraphAction};
    let cmd = Commands::Mem {
        action: MemAction::Graph {
            action: MemGraphAction::Hubs {
                threshold: 20,
                sample: 30,
            },
        },
    };
    let (m, reason, b) = cli_command_to_api(&cmd);
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("local-only"), "reason: {reason}");
    assert!(b.is_none());
}

#[test]
fn mem_update_relays_changed_by() {
    use crate::MemAction;
    let cmd = Commands::Mem {
        action: MemAction::Update {
            id: "fact_x".into(),
            title: Some("New title".into()),
            summary: None,
            area: None,
            tags: None,
            body: None,
            importance: None,
            changed_by: Some("cli:test".into()),
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "PATCH");
    assert_eq!(p, "/api/mem/items/fact_x");
    let body: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(body["title"], "New title");
    assert_eq!(body["changed_by"], "cli:test");
}

#[test]
fn mem_get_relays_to_items_id() {
    use crate::MemAction;
    let cmd = Commands::Mem {
        action: MemAction::Get {
            id: "fact_x".into(),
        },
    };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/mem/items/fact_x");
}

#[test]
fn mem_stats_still_routes_to_stats() {
    use crate::MemAction;
    let cmd = Commands::Mem {
        action: MemAction::Stats,
    };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/mem/stats");
}

#[test]
fn send_text_is_fire_and_forget_on_mesh() {
    let cmd = Commands::SendText {
        text: Some("ping".into()),
        pane: Some("abc123".into()),
        from_file: None,
        stdin: false,
        actions: None,
        delay: None,
        at: None,
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/panes/abc123/send");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert_eq!(body["text"], "ping");
    assert!(
        body.get("verify").is_none(),
        "send must never carry verify — the flag has been removed (#101)"
    );
}

#[test]
fn send_delay_maps_to_delayed_messages() {
    let cmd = Commands::SendText {
        text: Some("later".into()),
        pane: Some("abc123".into()),
        from_file: None,
        stdin: false,
        actions: None,
        delay: Some("1m".into()),
        at: None,
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/panes/abc123/delayed-messages");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert_eq!(body["text"], "later");
    assert!(
        body["scheduledAtMs"].as_u64().is_some(),
        "delayed send must carry scheduledAtMs"
    );
}

#[test]
fn send_text_stdin_mesh_body_preserves_exact_text() {
    let text = "it's got 'quotes' and \"doubles\"\nsecond line\n";
    let mut stdin = text.as_bytes();
    let (m, p, b) = super::api_map::send_text_to_api(
        &None,
        &Some("abc123".into()),
        &None,
        true,
        &None,
        &mut stdin,
    );
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/panes/abc123/send");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert_eq!(body["text"], text);
}

#[test]
fn send_actions_allows_empty_payload_on_mesh() {
    let empty = "";
    let mut stdin = empty.as_bytes();
    let actions = Some("enter".to_string());
    let (m, p, b) = super::api_map::send_text_to_api(
        &None,
        &Some("abc123".into()),
        &None,
        true,
        &actions,
        &mut stdin,
    );
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/panes/abc123/send");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert!(body.get("text").is_none());
    assert_eq!(body["actions"][0]["type"], "key");
    assert_eq!(body["actions"][0]["key"], "enter");
}

/// `web link` embeds the LOCAL token — relaying would mint a link with
/// the wrong credentials, so it must refuse honestly over --mesh.
#[test]
fn web_link_is_local_only_over_mesh() {
    let cmd = Commands::Web {
        action: crate::args::misc::WebAction::Link {
            peer: None,
            host: None,
            port: None,
            query: false,
        },
    };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(p.contains("local-only"));
}
