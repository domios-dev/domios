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
fn self_update_apply_maps_to_post_self_apply() {
    let (m, p, b) = cli_command_to_api(&Commands::SelfUpdate {
        check: false, apply: true, rollback: false, channel: None,
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/update/self_apply");
    assert!(b.is_none());
}

#[test]
fn self_update_apply_with_channel_appends_query() {
    let (m, p, _) = cli_command_to_api(&Commands::SelfUpdate {
        check: false, apply: true, rollback: false,
        channel: Some("rc".to_string()),
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/update/self_apply?channel=rc");
}

#[test]
fn self_update_check_maps_to_check_endpoint() {
    let (m, p, _) = cli_command_to_api(&Commands::SelfUpdate {
        check: true, apply: false, rollback: false, channel: None,
    });
    assert_eq!(m, "GET");
    assert_eq!(p, "/api/update/check");
}

#[test]
fn self_update_rollback_maps_to_rollback_endpoint() {
    let (m, p, _) = cli_command_to_api(&Commands::SelfUpdate {
        check: false, apply: false, rollback: true, channel: None,
    });
    assert_eq!(m, "POST");
    assert_eq!(p, "/api/update/rollback");
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
