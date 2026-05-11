use super::api_map::cli_command_to_api;
use super::parse_ip_port;
use super::{MeshAction, MeshPeerAction};
use crate::{Commands, UpdateAction};

fn update_cmd(
    action: Option<UpdateAction>,
    check: bool,
    apply: bool,
    channel: Option<String>,
    manifest_url: Option<String>,
) -> Commands {
    Commands::Update { action, check, apply, channel, manifest_url }
}

#[test]
fn check_maps_to_get_check() {
    let (m, p, b) = cli_command_to_api(&update_cmd(None, true, false, None, None));
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/update/check");
    assert!(b.is_none());
}

#[test]
fn check_force_maps_to_query_param() {
    let (m, p, _) = cli_command_to_api(&update_cmd(
        Some(UpdateAction::Check { force: true }),
        false, false, None, None,
    ));
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/update/check?force=true");
}

#[test]
fn apply_maps_to_install_auto() {
    let (m, p, b) = cli_command_to_api(&update_cmd(None, false, true, None, None));
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/update/install?method=auto");
    assert!(b.is_none());
}

#[test]
fn apply_with_manifest_url_appends_query() {
    let (m, p, _) = cli_command_to_api(&update_cmd(
        None, false, true, None,
        Some("http://localhost:9999/latest.json".to_string()),
    ));
    assert_eq!(m, "POST");
    assert!(p.starts_with("/api/update/install?method=auto&manifest_url="));
    assert!(p.contains("localhost%3A9999"));
}

#[test]
fn channel_get_maps_to_get_channel() {
    let (m, p, b) = cli_command_to_api(&update_cmd(
        None, false, false, Some(String::new()), None,
    ));
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/update/channel");
    assert!(b.is_none());
}

#[test]
fn channel_set_maps_to_post_channel_with_body() {
    let (m, p, b) = cli_command_to_api(&update_cmd(
        None, false, false, Some("rc".to_string()), None,
    ));
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/update/channel");
    let body = b.expect("body");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["channel"], "rc");
}

#[test]
fn mesh_refresh_maps_to_post_refresh() {
    let (m, p, b) = cli_command_to_api(&Commands::Mesh { action: MeshAction::Refresh });
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
        action: MeshAction::Announce { to: Some("BLACKSTAR".into()) },
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/announce/all?to=BLACKSTAR");
}

#[test]
fn parse_ip_port_plain_ipv4() {
    assert_eq!(parse_ip_port("192.168.1.1"), ("192.168.1.1".to_string(), None));
}

#[test]
fn parse_ip_port_ipv4_with_port() {
    assert_eq!(parse_ip_port("100.84.1.2:7685"), ("100.84.1.2".to_string(), Some(7685)));
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
                addr: "100.84.159.37:7685".into(),
            },
        },
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/mesh/peers/BLACKSTAR/addr");
    let body = b.expect("body");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ip"], "100.84.159.37");
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
    assert!(p.contains("q=windows%20release%20build"), "query missing/unencoded: {}", p);
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
fn mem_get_relays_to_items_id() {
    use crate::MemAction;
    let cmd = Commands::Mem { action: MemAction::Get { id: "fact_x".into() } };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/mem/items/fact_x");
}

#[test]
fn mem_stats_still_routes_to_stats() {
    use crate::MemAction;
    let cmd = Commands::Mem { action: MemAction::Stats };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/mem/stats");
}

#[test]
fn mem_local_only_falls_back_to_stats() {
    use crate::MemAction;
    let cmd = Commands::Mem {
        action: MemAction::RunWorker {
            kind: "dreaming-light".into(),
            db_path: None,
        },
    };
    let (m, p, _) = cli_command_to_api(&cmd);
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/mem/stats");
}

#[test]
fn send_text_defaults_to_fire_and_forget_on_mesh() {
    let cmd = Commands::SendText {
        text: "ping".into(),
        pane: Some("abc123".into()),
        from_file: None,
        from_stdin: false,
        verify: false,
        no_wait: false,
    };
    let (m, p, b) = cli_command_to_api(&cmd);
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/panes/abc123/send");
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert_eq!(body["text"], "ping");
    assert!(body.get("verify").is_none(), "default path should omit verify and let server use fire-and-forget");
}

#[test]
fn send_text_verify_true_is_forwarded_on_mesh() {
    let cmd = Commands::SendText {
        text: "ping".into(),
        pane: Some("abc123".into()),
        from_file: None,
        from_stdin: false,
        verify: true,
        no_wait: false,
    };
    let (_, _, b) = cli_command_to_api(&cmd);
    let body: serde_json::Value = serde_json::from_str(&b.expect("body")).unwrap();
    assert_eq!(body["verify"], true);
}
