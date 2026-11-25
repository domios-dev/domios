use super::*;

#[test]
fn local_web_link_prefers_https_serve_url() {
    let status = serde_json::json!({
        "tailscale_https_url": "https://host.ts.net",
        "tailscale_ip": "100.64.0.5",
        "local_ip": "192.168.1.10",
        "api_port": 7685
    });
    let link = build_local_web_link(Some(&status), None, 9999, false, "tok", false);
    assert_eq!(link.url, "https://host.ts.net/#token=tok");
    assert_eq!(link.host, "host.ts.net");
    assert_eq!(link.port, 443);
}

#[test]
fn local_web_link_preserves_non_default_https_port() {
    let status = serde_json::json!({
        "tailscale_https_url": "https://host.ts.net:8443",
        "api_port": 7685
    });
    let link = build_local_web_link(Some(&status), None, 9999, false, "tok", false);
    assert_eq!(link.url, "https://host.ts.net:8443/#token=tok");
    assert_eq!(link.port, 8443);
}

#[test]
fn local_web_link_falls_back_to_tailscale_ip() {
    let status = serde_json::json!({
        "tailscale_ip": "100.64.0.5",
        "local_ip": "192.168.1.10",
        "api_port": 7685
    });
    let link = build_local_web_link(Some(&status), None, 9999, false, "tok", false);
    assert_eq!(link.url, "http://100.64.0.5:7685/#token=tok");
    assert_eq!(link.host, "100.64.0.5");
    assert_eq!(link.port, 7685);
}

#[test]
fn local_web_link_host_override_keeps_http_shape() {
    let status = serde_json::json!({
        "tailscale_https_url": "https://host.ts.net",
        "api_port": 7685
    });
    let link = build_local_web_link(Some(&status), Some("10.0.0.8"), 7777, true, "tok", false);
    assert_eq!(link.url, "http://10.0.0.8:7777/#token=tok");
    assert_eq!(link.host, "10.0.0.8");
    assert_eq!(link.port, 7777);
}

#[test]
fn local_web_link_query_token_form() {
    let link = build_local_web_link(None, None, 7685, false, "tok", true);
    assert_eq!(link.url, "http://127.0.0.1:7685/?token=tok");
}

#[test]
fn local_web_link_port_override_suppresses_https_but_keeps_host_priority() {
    let status = serde_json::json!({
        "tailscale_https_url": "https://host.ts.net",
        "tailscale_ip": "100.64.0.5",
        "api_port": 7685
    });
    let link = build_local_web_link(Some(&status), None, 7777, true, "tok", false);
    assert_eq!(link.url, "http://100.64.0.5:7777/#token=tok");
    assert_eq!(link.port, 7777);
}

#[test]
fn peer_lookup_accepts_installation_id_prefix_instance_id_and_names() {
    let peers = serde_json::json!({
        "paired": [{
            "instance_id": "66c926f1-aaaa-bbbb-cccc-000000000001",
            "instance_name": "acceptance-r161 [dev-r161]",
            "online": true
        }],
        "installations": [{
            "installation_id": "7a835b57-1111-2222-3333-000000000001",
            "instance_id": "66c926f1-aaaa-bbbb-cccc-000000000001",
            "name": "acceptance-r161",
            "active": true,
            "online": true
        }]
    });
    for target in [
        "7a835b57-1111-2222-3333-000000000001",
        "7a835b57",
        "66c926f1-aaaa-bbbb-cccc-000000000001",
        "66c926f1",
        "acceptance-r161",
        "acceptance-r161 [dev-r161]",
    ] {
        assert_eq!(
            resolve_peer_instance_id_from_response(&peers, target).as_deref(),
            Some("66c926f1-aaaa-bbbb-cccc-000000000001"),
            "{target}"
        );
    }
}

#[test]
fn peer_lookup_refuses_cross_machine_collisions_across_stages() {
    // Machine A renamed "studio" in registry while offline; Machine B advertises "studio" online.
    let cross_machine = serde_json::json!({
        "paired": [
            {
                "instance_id": "peer-a",
                "instance_name": "old-a",
                "online": false
            },
            {
                "instance_id": "peer-b",
                "instance_name": "studio",
                "online": true
            }
        ],
        "installations": [
            {
                "installation_id": "7a835b57-inst-a",
                "instance_id": "peer-a",
                "name": "studio",
                "active": true,
                "online": false
            },
            {
                "installation_id": "99999999-inst-b",
                "instance_id": "peer-b",
                "name": "workstation",
                "active": true,
                "online": true
            }
        ]
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&cross_machine, "studio"),
        None,
        "offline registry name on machine A vs online advertised name on machine B must refuse"
    );

    // Cross-stage prefix collision: machine A's installation_id prefix vs machine B's instance_id prefix.
    let prefix_collision = serde_json::json!({
        "paired": [
            {
                "instance_id": "11111111-peer-a",
                "instance_name": "alpha",
                "online": false
            },
            {
                "instance_id": "7a835b57-peer-b",
                "instance_name": "beta",
                "online": true
            }
        ],
        "installations": [
            {
                "installation_id": "7a835b57-inst-a",
                "instance_id": "11111111-peer-a",
                "name": "alpha",
                "active": true,
                "online": false
            },
            {
                "installation_id": "99999999-inst-b",
                "instance_id": "7a835b57-peer-b",
                "name": "beta",
                "active": true,
                "online": true
            }
        ]
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&prefix_collision, "7a835b57"),
        None,
        "installation_id prefix on machine A vs instance_id prefix on machine B must refuse"
    );

    // Offline active registry installation with null instance_id vs online peer with same name.
    let unlinked_offline_vs_online = serde_json::json!({
        "paired": [
            {
                "instance_id": "peer-b",
                "instance_name": "studio",
                "online": true
            }
        ],
        "installations": [
            {
                "installation_id": "inst-a",
                "instance_id": null,
                "name": "studio",
                "active": true,
                "online": false
            },
            {
                "installation_id": "inst-b",
                "instance_id": "peer-b",
                "name": "studio",
                "active": true,
                "online": true
            }
        ]
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&unlinked_offline_vs_online, "studio"),
        None
    );
}

#[test]
fn peer_lookup_prefers_online_for_stale_duplicate_of_same_machine() {
    // codeterm#434 without installation ids: stale offline duplicate does not hide live peer.
    let legacy_dup = serde_json::json!({
        "paired": [
            {
                "instance_id": "a4db6f3d-dead",
                "instance_name": "BLACKSTAR",
                "online": false
            },
            {
                "instance_id": "2556b3f9-live",
                "instance_name": "BLACKSTAR",
                "online": true
            }
        ],
        "installations": []
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&legacy_dup, "BLACKSTAR").as_deref(),
        Some("2556b3f9-live")
    );

    // codeterm#434 with matching installation id on both records.
    let same_inst_dup = serde_json::json!({
        "paired": [
            {
                "instance_id": "a4db6f3d-dead",
                "instance_name": "BLACKSTAR",
                "online": false
            },
            {
                "instance_id": "2556b3f9-live",
                "instance_name": "BLACKSTAR",
                "online": true
            }
        ],
        "installations": [
            {
                "installation_id": "inst-blackstar",
                "instance_id": "a4db6f3d-dead",
                "name": "BLACKSTAR",
                "active": true,
                "online": false
            },
            {
                "installation_id": "inst-blackstar",
                "instance_id": "2556b3f9-live",
                "name": "BLACKSTAR",
                "active": true,
                "online": true
            }
        ]
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&same_inst_dup, "BLACKSTAR").as_deref(),
        Some("2556b3f9-live")
    );
}

#[test]
fn peer_lookup_prefers_exact_prod_name_over_dev_suffixed_peer() {
    let prod_and_dev = serde_json::json!({
        "paired": [
            {
                "instance_id": "peer-prod",
                "instance_name": "studio",
                "online": true
            },
            {
                "instance_id": "peer-dev",
                "instance_name": "studio [dev-qa]",
                "online": true
            }
        ],
        "installations": [
            {
                "installation_id": "inst-prod",
                "instance_id": "peer-prod",
                "name": "studio",
                "active": true,
                "online": true
            },
            {
                "installation_id": "inst-dev",
                "instance_id": "peer-dev",
                "name": "studio [dev-qa]",
                "active": true,
                "online": true
            }
        ]
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&prod_and_dev, "studio").as_deref(),
        Some("peer-prod"),
        "exact prod name must beat DEV-suffixed peer sharing configured name"
    );
    assert_eq!(
        resolve_peer_instance_id_from_response(&prod_and_dev, "studio [dev-qa]").as_deref(),
        Some("peer-dev"),
        "explicit DEV-suffixed name resolves to DEV peer"
    );

    // Without an exact prod peer, configured-name fallback still refuses two distinct DEV peers.
    let two_devs = serde_json::json!({
        "paired": [
            {
                "instance_id": "peer-dev-a",
                "instance_name": "studio [dev-a]",
                "online": false
            },
            {
                "instance_id": "peer-dev-b",
                "instance_name": "studio [dev-b]",
                "online": true
            }
        ],
        "installations": [
            {
                "installation_id": "inst-dev-a",
                "instance_id": "peer-dev-a",
                "name": "studio [dev-a]",
                "active": true,
                "online": false
            },
            {
                "installation_id": "inst-dev-b",
                "instance_id": "peer-dev-b",
                "name": "studio [dev-b]",
                "active": true,
                "online": true
            }
        ]
    });
    assert_eq!(
        resolve_peer_instance_id_from_response(&two_devs, "studio"),
        None
    );
}
