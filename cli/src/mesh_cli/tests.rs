use super::api_map::cli_command_to_api;
use super::parse_ip_port;
use super::{MeshAction, MeshPeerAction};
use crate::{BridgeAction, Commands, TabAction, UpdateAction};

mod delayed;

fn update_cmd(action: Option<UpdateAction>, manifest_url: Option<String>) -> Commands {
    Commands::Update {
        action,
        manifest_url,
        after_update: None,
    }
}

#[test]
fn peer_api_url_skips_loopback_addresses() {
    let peer = serde_json::json!({
        "instance_name": "eight",
        "api_port": 7685,
        "addresses": [
            {"ip": "127.0.0.1", "is_tailscale": false},
            {"ip": "100.100.100.100", "is_tailscale": true}
        ]
    });

    assert_eq!(
        super::relay::peer_api_url(&peer).as_deref(),
        Some("http://100.100.100.100:7685")
    );
}

#[test]
fn tailscale_hint_fires_only_for_tailscale_addresses() {
    let hint = |url: &str| super::relay::tailscale_unreachable_hint(url);
    assert!(
        hint("http://100.64.10.2:7685").is_some(),
        "CGNAT 100.64/10"
    );
    assert!(
        hint("https://blackstar.example-tailnet.ts.net").is_some(),
        "ts.net host"
    );
    assert!(
        hint("http://100.100.100.100:7685").is_some(),
        "CGNAT with port"
    );
    assert!(
        hint("http://192.168.1.5:7685").is_none(),
        "LAN IP is not Tailscale"
    );
    assert!(
        hint("http://100.200.1.1:7685").is_none(),
        "100.200 is outside 100.64/10"
    );
    assert!(hint("http://10.0.0.1:7685").is_none(), "private range");
    assert!(
        hint("http://100.64.10.2:7685")
            .unwrap()
            .contains("tailscale ping"),
        "hint names the remediation"
    );
    assert!(
        hint("Network error: error sending request for url (http://100.64.10.2:7685/api/mesh/relay)")
            .is_some(),
        "detects a Tailscale address inside a daemon error body"
    );
    assert!(
        hint("HTTP 500: internal server error, no address in this body").is_none(),
        "plain 5xx text without a Tailscale address stays silent"
    );
}

#[test]
fn peer_api_url_refuses_loopback_only_peer() {
    let peer = serde_json::json!({
        "instance_name": "eight",
        "api_port": 7685,
        "addresses": [
            {"ip": "127.0.0.1", "is_tailscale": false},
            {"ip": "::1", "is_tailscale": false}
        ]
    });

    assert_eq!(super::relay::peer_api_url(&peer), None);
}

#[test]
fn peer_api_url_brackets_ipv6_host() {
    let peer = serde_json::json!({
        "api_port": 7686,
        "addresses": [
            {"ip": "fd7a:115c:a1e0::1", "is_tailscale": true}
        ]
    });

    assert_eq!(
        super::relay::peer_api_url(&peer).as_deref(),
        Some("http://[fd7a:115c:a1e0::1]:7686")
    );
}

#[test]
fn paste_put_over_mesh_maps_to_remote_pasteboard_create() {
    let cmd = Commands::Paste {
        action: crate::PasteAction::Put {
            body: Some("payload".into()),
            from_file: None,
            from_pad: None,
            from_stdin: false,
            ttl: Some("5m".into()),
            content_type: Some("text/plain".into()),
            pop: true,
        },
    };

    let (method, path, body) = cli_command_to_api(&cmd);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/pasteboard");
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();
    assert_eq!(body["body"], "payload");
    assert_eq!(body["ttl_ms"], 300_000);
    assert_eq!(body["content_type"], "text/plain");
    assert_eq!(body["pop_on_read"], true);
}

#[test]
fn paste_get_and_list_over_mesh_map_to_remote_pasteboard() {
    let (method, path, body) = cli_command_to_api(&Commands::Paste {
        action: crate::PasteAction::Get {
            id: "paste:pcze9w".into(),
            pop: true,
        },
    });
    assert_eq!(method, "GET");
    assert_eq!(path, "/api/pasteboard/paste:pcze9w?pop=true");
    assert!(body.is_none());

    let (method, path, body) = cli_command_to_api(&Commands::Paste {
        action: crate::PasteAction::List,
    });
    assert_eq!(method, "GET");
    assert_eq!(path, "/api/pasteboard");
    assert!(body.is_none());
}

#[test]
fn bridge_send_over_mesh_decodes_quoted_newline() {
    let (method, path, body) = cli_command_to_api(&Commands::Bridge {
        action: BridgeAction::Send {
            text: Some("first\\nsecond".into()),
            from_pad: None,
            raw_text: false,
            service: "telegram".into(),
            to: 42,
            append: false,
            r#type: "text".into(),
            file: None,
            caption: None,
            filename: None,
            buttons: None,
        },
    });

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/bridge/send");
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();
    assert_eq!(body["text"], "first\nsecond");
}

#[test]
fn remote_worker_record_body_builds_from_spawn_request_and_response() {
    use super::relay::remote_worker_record_body;
    let spawn_body = serde_json::json!({
        "agent": "claude",
        "report_to": "4d479b6c",
        "role": "Worker",
        "model": "opus",
    });
    let spawn_data = serde_json::json!({
        "tab_id": "53854ac8",
        "title": "Canvas Orchestrator",
    });
    let reg = remote_worker_record_body("eight", &spawn_body, &spawn_data)
        .expect("registerable spawn produces a record body");
    assert_eq!(reg["orchestrator_tab"], "4d479b6c");
    assert_eq!(reg["peer"], "eight");
    assert_eq!(reg["remote_tab"], "53854ac8");
    assert_eq!(reg["title"], "Canvas Orchestrator");
    assert_eq!(reg["model"], "opus");
}

#[test]
fn remote_worker_record_body_skips_without_report_target_or_tab() {
    use super::relay::remote_worker_record_body;
    // No report target → nothing to nest under.
    let no_report = serde_json::json!({"agent": "claude"});
    let data = serde_json::json!({"tab_id": "p1", "title": "W"});
    assert!(remote_worker_record_body("eight", &no_report, &data).is_none());

    // Spawn failed (no tab id) → nothing to register.
    let with_report = serde_json::json!({"report_to": "main"});
    let no_tab = serde_json::json!({"error": "boom"});
    assert!(remote_worker_record_body("eight", &with_report, &no_tab).is_none());
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
            continue_tab: None,
            no_continue: false,
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
            continue_tab: None,
            no_continue: false,
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
            continue_tab: Some("tab_1".into()),
            no_continue: false,
            after_update: None,
        }),
        None,
    ));
    assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("local-only"), "reason: {reason}");
    assert!(b.is_none());
}

/// Persona reads the serving daemon's compiled prompts and its own overlay
/// directory, so a mesh call must refuse rather than answer from local state.
#[test]
fn persona_inspection_is_refused_over_mesh_for_every_subcommand() {
    use crate::args::persona::PersonaAction;

    for action in [
        PersonaAction::List,
        PersonaAction::Reload,
        PersonaAction::Show {
            role: "Worker".into(),
            kind: Some("rust-backend-dev".into()),
            guidance: None,
            occasion: None,
        },
    ] {
        let (m, reason, b) = cli_command_to_api(&Commands::Persona { action });
        assert_eq!(m, super::api_map::UNSUPPORTED_OVER_MESH);
        assert!(reason.contains("socket-only"), "reason: {reason}");
        assert!(
            reason.contains("overlay directory"),
            "the refusal must say whose catalog would be wrong: {reason}"
        );
        assert!(b.is_none(), "a refused command carries no body");
    }
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
                addr: "100.100.100.100:7685".into(),
            },
        },
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/peers/BLACKSTAR/addr");
    let body = b.expect("body");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ip"], "100.100.100.100");
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
            project: None,
            limit: 5,
            rerank: false,
            decision_rerank: None,
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
            body_file: None,
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
            team: None,
            no_team: false,
            agent: "claude".into(),
            model: None,
            reasoning_effort: Some("high".into()),
            cwd: None,
            skill: None,
            description: None,
            report_tab: Some("tab_9".into()),
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
    assert_eq!(body["report_target_tab_id"], "tab_9");
    assert_eq!(body["reasoning_effort"], "high");
}

#[test]
fn tab_new_relays_cmd_field() {
    use crate::TabAction;
    let cmd = Commands::Tab {
        action: TabAction::New {
            workspace: Some("ws_1".into()),
            cwd: Some("C:/Developer/codeterm".into()),
            title: Some("runner".into()),
            team: None,
            no_team: false,
            cmd: Some("pnpm test".into()),
            eager: true,
            chat_backend: None,
            config: None,
        },
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/tabs");
    let body: serde_json::Value = serde_json::from_str(&b.unwrap()).unwrap();
    assert_eq!(body["workspace_id"], "ws_1");
    assert_eq!(body["cwd"], "C:/Developer/codeterm");
    assert_eq!(body["title"], "runner");
    assert_eq!(body["cmd"], "pnpm test");
    assert_eq!(body["eager"], true);
}

#[test]
fn tab_list_and_inspect_relay_compact_scope_and_explicit_detail_ids() {
    use crate::TabAction;
    let list = Commands::Tab {
        action: TabAction::List {
            workspace: Some("ws 1".into()),
            all_workspaces: false,
            agents: true,
        },
    };
    let (method, path, body) = cli_command_to_api(&list);
    assert_eq!(method, "GET");
    assert_eq!(path, "/api/tabs?agents=true&workspace_id=ws%201");
    assert!(body.is_none());

    let inspect = Commands::Tab {
        action: TabAction::Inspect {
            tabs: vec!["tab_1".into(), "tab_2".into()],
            tab: vec![],
            lifecycle: false,
        },
    };
    let (method, path, body) = cli_command_to_api(&inspect);
    assert_eq!(method, "GET");
    assert_eq!(path, "/api/tabs/inspect?tab_ids=tab_1,tab_2");
    assert!(body.is_none());

    let lifecycle = Commands::Tab {
        action: TabAction::Inspect {
            tabs: vec![],
            tab: vec!["tab_1".into()],
            lifecycle: true,
        },
    };
    let (_, path, _) = cli_command_to_api(&lifecycle);
    assert_eq!(path, "/api/tabs/inspect?tab_ids=tab_1&lifecycle=true");
}

#[test]
fn explicit_address_scope_mesh_all_workspaces_reaches_every_peer_workspace() {
    let command = Commands::Tab {
        action: TabAction::List {
            workspace: None,
            all_workspaces: true,
            agents: false,
        },
    };

    let (method, path, body) = cli_command_to_api(&command);

    assert_eq!(method, "GET");
    assert_eq!(path, "/api/tabs?agents=false&all_workspaces=true");
    assert!(body.is_none());
}

#[test]
fn tab_list_defaults_to_all_workspaces_over_mesh() {
    let (method, path, body) = cli_command_to_api(&tab_list_command());

    assert_eq!(method, "GET");
    assert_eq!(path, "/api/tabs?agents=false");
    assert!(body.is_none());
}

#[test]
fn tab_respond_maps_to_provider_prompt_route() {
    let command = Commands::Tab {
        action: TabAction::Respond {
            option: 2,
            tab: Some("child_7".into()),
        },
    };

    let (method, path, body) = cli_command_to_api(&command);
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/child_7/prompt/respond");
    assert_eq!(body["value"], "2");
}

#[test]
fn plugin_with_mesh_never_reaches_direct_local_dispatch() {
    let command = Commands::Plugin {
        action: crate::PluginAction::List,
    };

    assert!(crate::transport::should_dispatch_direct_local(
        &command, None
    ));
    assert!(!crate::transport::should_dispatch_direct_local(
        &command,
        Some("peer")
    ));
    let (method, reason, body) = cli_command_to_api(&command);
    assert_eq!(method, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("local-only"));
    assert!(body.is_none());
}

#[test]
fn system_and_docs_refuse_instead_of_identifying_the_peer() {
    let commands = [
        Commands::System {
            action: crate::SystemAction::AutostartStatus,
        },
        Commands::Docs { name: None },
    ];

    for command in commands {
        let (method, reason, body) = cli_command_to_api(&command);
        assert_eq!(method, super::api_map::UNSUPPORTED_OVER_MESH);
        assert!(!reason.contains("/api/identify"));
        assert!(body.is_none());
    }
}

#[test]
fn workspace_create_over_mesh_preserves_cwd() {
    let command = Commands::Workspace {
        action: crate::WorkspaceAction::Create {
            name: "remote".into(),
            cwd: Some("D:/Developer/project".into()),
            provision: false,
        },
    };

    let (method, path, body) = cli_command_to_api(&command);
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/workspaces");
    assert_eq!(body["name"], "remote");
    assert_eq!(body["cwd"], "D:/Developer/project");
    assert!(body.get("provision").is_none());
}

#[test]
fn workspace_create_over_mesh_carries_provision() {
    let command = Commands::Workspace {
        action: crate::WorkspaceAction::Create {
            name: "bench".into(),
            cwd: None,
            provision: true,
        },
    };

    let (method, path, body) = cli_command_to_api(&command);
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/workspaces");
    assert_eq!(body["provision"], true);
    assert!(body.get("cwd").is_none());
}

#[test]
fn workspace_move_tab_over_mesh_uses_the_mutation_route() {
    let command = Commands::Workspace {
        action: crate::WorkspaceAction::MoveTab {
            tab_id: "tab_1".into(),
            from: "ws_from".into(),
            to: "ws_to".into(),
        },
    };

    let (method, path, body) = cli_command_to_api(&command);
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/workspaces/ws_from/tabs/tab_1/move");
    assert_eq!(body["to_workspace_id"], "ws_to");
}

#[test]
fn workspace_current_over_mesh_refuses_instead_of_listing() {
    let command = Commands::Workspace {
        action: crate::WorkspaceAction::Current { workspace: None },
    };

    let (method, reason, body) = cli_command_to_api(&command);

    assert_eq!(method, super::api_map::UNSUPPORTED_OVER_MESH);
    assert!(reason.contains("caller-local"));
    assert!(body.is_none());
}

#[test]
fn remote_worker_forget_body_matches_registry_delete_contract() {
    let body = super::relay::remote_worker_forget_body("ROG", "tab_1");
    assert_eq!(body["peer"], "ROG");
    assert_eq!(body["remote_tab"], "tab_1");
    assert!(body.get("orchestrator_tab").is_none());
}

#[test]
fn canonical_tab_close_preserves_remote_worker_cleanup_route() {
    let mut action = TabAction::Close {
        id: None,
        tab: Some("tab_remote".into()),
        force: false,
    };
    crate::args::misc::normalize_required_tab_selector(&mut action).unwrap();
    let command = Commands::Tab { action };
    assert_eq!(super::remote_close_tab_id(&command), Some("tab_remote"));
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
            body_file: None,
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
        tab: Some("abc123".into()),
        from_file: None,
        from_pad: None,
        stdin: false,
        actions: None,
        delay: None,
        at: None,
        every: None,
        until: None,
        count: None,
        require_agent: false,
        to_shell: false,
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/tabs/abc123/send");
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
        tab: Some("abc123".into()),
        from_file: None,
        from_pad: None,
        stdin: false,
        actions: None,
        delay: Some("1m".into()),
        at: None,
        every: None,
        until: None,
        count: None,
        require_agent: false,
        to_shell: false,
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/tabs/abc123/delayed-messages");
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
        &None,
        true,
        &None,
        &mut stdin,
    );
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/tabs/abc123/send");
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
        &None,
        true,
        &actions,
        &mut stdin,
    );
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/tabs/abc123/send");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert!(body.get("text").is_none());
    assert_eq!(body["actions"][0]["type"], "key");
    assert_eq!(body["actions"][0]["key"], "enter");
}

#[test]
fn only_post_sends_are_delivery_id_bearing() {
    assert!(super::relay::is_send_relay("POST", "/api/tabs/abc123/send"));
    assert!(super::relay::is_send_relay(
        "post",
        "/api/tabs/general/send"
    ));
    assert!(
        !super::relay::is_send_relay("GET", "/api/tabs/abc123/send"),
        "a GET is never a logical send"
    );
    assert!(!super::relay::is_send_relay("POST", "/api/tabs"));
}

#[test]
fn delivery_id_prefers_explicit_env_override() {
    assert_eq!(
        super::relay::delivery_id_or_generate(Some("pinned-key-123".to_string())),
        "pinned-key-123",
        "an explicit id is reused verbatim so retries dedupe"
    );
}

#[test]
fn delivery_id_generates_uuid_when_unset_or_blank() {
    let generated = super::relay::delivery_id_or_generate(None);
    assert!(
        uuid::Uuid::parse_str(&generated).is_ok(),
        "unset env yields a fresh v4 uuid: {generated}"
    );
    let from_blank = super::relay::delivery_id_or_generate(Some("   ".to_string()));
    assert!(
        uuid::Uuid::parse_str(&from_blank).is_ok(),
        "blank env falls back to a generated uuid: {from_blank}"
    );
}

#[test]
fn relay_envelope_stamps_delivery_id_only_on_sends() {
    let send =
        super::relay::build_relay_envelope("POST", "/api/tabs/abc/send", Some(r#"{"text":"hi"}"#));
    let id = send
        .get("delivery_id")
        .and_then(|v| v.as_str())
        .expect("send envelope carries a delivery_id");
    assert!(!id.is_empty());
    assert_eq!(send["body"]["text"], "hi", "body is preserved");

    let read = super::relay::build_relay_envelope("GET", "/api/tabs", None);
    assert!(
        read.get("delivery_id").is_none(),
        "reads carry no delivery_id"
    );
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

fn mesh_cli(json: bool, command: Commands) -> crate::Cli {
    crate::Cli {
        json,
        mesh: Some("BLACKSTAR".into()),
        command,
    }
}

fn tab_list_command() -> Commands {
    Commands::Tab {
        action: TabAction::List {
            workspace: None,
            all_workspaces: false,
            agents: false,
        },
    }
}

/// The `--json` rung is the one under test: it skips the human-tree guard, and
/// used to fall through to the verbatim generic relay (the peer's bare array).
#[test]
fn mesh_tab_list_json_is_formatted_locally_instead_of_relayed_verbatim() {
    use super::tab_list::{rendering_for, TabListRendering};

    let json_mode = mesh_cli(true, tab_list_command());
    assert!(json_mode.json, "fixture must reach the --json branch");
    assert_eq!(
        rendering_for(&json_mode),
        Some(TabListRendering::JsonEnvelope)
    );

    let human = mesh_cli(false, tab_list_command());
    assert_eq!(rendering_for(&human), Some(TabListRendering::Tree));

    let inspect = mesh_cli(
        true,
        Commands::Tab {
            action: TabAction::Inspect {
                tabs: vec![],
                tab: vec!["tab_1".into()],
                lifecycle: false,
            },
        },
    );
    assert_eq!(
        rendering_for(&inspect),
        None,
        "only tab list is formatted locally; everything else stays on the relay"
    );
}

#[test]
fn mesh_tab_list_json_emits_the_same_envelope_as_the_local_path() {
    use crate::cmd::print::{tab_list_json_envelope, uniform_id_value, TAB_LIST_JSON_KEY};

    let rest_body = serde_json::json!([
        {"id": "tab_1", "title": "one"},
        {"id": "tab_2", "title": "two"},
    ]);

    let over_mesh = uniform_id_value(&super::tab_list::json_envelope(&rest_body));
    let local = uniform_id_value(&tab_list_json_envelope(
        rest_body.as_array().unwrap().clone(),
    ));

    assert_eq!(
        over_mesh, local,
        "one command must not return two JSON shapes depending on transport"
    );
    let tabs = over_mesh[TAB_LIST_JSON_KEY]
        .as_array()
        .expect("the printed envelope carries the tabs array");
    assert_eq!(tabs.len(), 2);
}

/// Normalizing at the CLI boundary must not migrate the REST contract: the
/// relayed `GET /api/tabs` body stays the bare array its consumers read.
#[test]
fn mesh_tab_list_json_leaves_the_relayed_rest_body_a_bare_array() {
    use crate::cmd::print::TAB_LIST_JSON_KEY;

    let rest_body = serde_json::json!([{"id": "tab_1", "title": "one"}]);
    let printed = super::tab_list::json_envelope(&rest_body);

    assert!(
        rest_body.is_array(),
        "GET /api/tabs keeps its bare-array response: {rest_body}"
    );
    assert!(rest_body.get(TAB_LIST_JSON_KEY).is_none());
    assert_eq!(printed[TAB_LIST_JSON_KEY], rest_body);
    assert_eq!(
        super::tab_list::json_envelope(&serde_json::Value::Null)[TAB_LIST_JSON_KEY],
        serde_json::json!([]),
        "an absent peer body still carries the tabs key"
    );
}

fn parse_spawn_command(args: &[&str]) -> Commands {
    let mut full = vec!["codeterm", "agent", "spawn"];
    full.extend_from_slice(args);
    <crate::Cli as clap::Parser>::try_parse_from(full)
        .expect("agent spawn must parse")
        .command
}

/// A generic task-less mesh Worker still relies on `role` for its Worker
/// doctrine; plan-bound dispatch uses the separate origin-owned protocol.
#[test]
fn a_task_less_mesh_worker_relays_its_role_to_the_peer() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let (method, path, body) = cli_command_to_api(&parse_spawn_command(&[
        "claude",
        "--cwd",
        "D:/peer/workspace",
        "--role",
        "Worker",
        "--task",
        "check out the repo, build it, serve it",
    ]));

    assert_eq!(method, "POST");
    assert_eq!(path, "/api/agents/spawn");
    let body: serde_json::Value =
        serde_json::from_str(&body.expect("agent spawn carries a body")).unwrap();
    assert_eq!(body["role"], "Worker");
    assert!(
        body.get("plan_task").is_none() && body.get("task_id").is_none(),
        "the mesh Worker owns no binding: {body}"
    );
    assert!(
        body.get("execution_intent").is_none(),
        "no declared intent is its own shape: {body}"
    );
}
#[path = "tests/mesh_spawn_close.rs"]
mod mesh_spawn_close;
