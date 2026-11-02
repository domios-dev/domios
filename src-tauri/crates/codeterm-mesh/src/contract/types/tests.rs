use super::*;

#[test]
fn relay_protocol_refuses_v1_and_accepts_v2_records() {
    assert_eq!(RELAY_PROTOCOL_VERSION, 2);
    assert!(!is_supported_relay_protocol(1));
    assert!(is_supported_relay_protocol(2));
    assert!(!is_supported_relay_protocol(0));
    assert!(!is_supported_relay_protocol(3));
}

#[test]
fn tailscale_cgnat_recognized() {
    assert!(is_tailscale_ip("100.64.0.1"));
    assert!(is_tailscale_ip("100.65.0.10"));
    assert!(is_tailscale_ip("100.127.255.254"));
    assert!(!is_tailscale_ip("100.63.255.255"));
    assert!(!is_tailscale_ip("100.128.0.0"));
}

#[test]
fn lan_private_ranges() {
    assert!(is_lan_private_ip("10.0.0.1"));
    assert!(is_lan_private_ip("172.16.0.1"));
    assert!(is_lan_private_ip("172.31.255.255"));
    assert!(is_lan_private_ip("192.168.1.1"));
    assert!(is_lan_private_ip("127.0.0.1"));
    assert!(!is_lan_private_ip("172.15.0.1"));
    assert!(!is_lan_private_ip("172.32.0.1"));
    assert!(!is_lan_private_ip("8.8.8.8"));
    assert!(!is_lan_private_ip("100.65.0.20")); // tailscale not LAN
}

#[test]
fn address_tier_orders_tailscale_first_then_lan_then_public() {
    let ts = PeerAddress {
        ip: "100.65.0.30".into(),
        is_tailscale: true,
    };
    let lan = PeerAddress {
        ip: "192.168.1.2".into(),
        is_tailscale: false,
    };
    let pub_ip = PeerAddress {
        ip: "8.8.8.8".into(),
        is_tailscale: false,
    };
    assert!(address_tier(&ts) < address_tier(&lan));
    assert!(address_tier(&lan) < address_tier(&pub_ip));
}

#[test]
fn address_tier_min_picks_tailscale() {
    let addrs = [
        PeerAddress {
            ip: "8.8.8.8".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "192.168.1.2".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "100.65.0.30".into(),
            is_tailscale: true,
        },
    ];
    let pick = addrs.iter().min_by_key(|a| address_tier(a)).unwrap();
    assert_eq!(pick.ip, "100.65.0.30");
}

#[test]
fn sanitize_addresses_passes_clean_ips_through() {
    let addrs = vec![
        PeerAddress {
            ip: "100.65.0.30".into(),
            is_tailscale: true,
        },
        PeerAddress {
            ip: "192.168.1.2".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "::1".into(),
            is_tailscale: false,
        },
    ];
    let out = sanitize_addresses(&addrs);
    assert_eq!(out.len(), 3);
}

#[test]
fn sanitize_addresses_drops_unparseable_strings() {
    // The actual poison strings observed in the wild from a broken `tailscale ip -4` GUI bridge: error text on stdout instead of an IP.
    let poisoned = vec![
        PeerAddress {
            ip: "Tailscale daemon is not running.".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "failed to connect to local backend".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "not-an-ip".into(),
            is_tailscale: false,
        },
    ];
    let out = sanitize_addresses(&poisoned);
    assert!(
        out.is_empty(),
        "all entries should be dropped, got: {:?}",
        out
    );
}

#[test]
fn sanitize_addresses_preserves_clean_drops_poisoned_in_mixed_list() {
    let mixed = vec![
        PeerAddress {
            ip: "100.65.0.30".into(),
            is_tailscale: true,
        },
        PeerAddress {
            ip: "Tailscale daemon is not running.".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "192.168.1.2".into(),
            is_tailscale: false,
        },
        PeerAddress {
            ip: "garbage".into(),
            is_tailscale: false,
        },
    ];
    let out = sanitize_addresses(&mixed);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].ip, "100.65.0.30");
    assert_eq!(out[1].ip, "192.168.1.2");
}

#[test]
fn proactive_sync_skips_when_offline() {
    // Offline peer — never sync, regardless of last_sync state.
    assert!(!should_proactive_sync(false, false, 1000, None, 60));
    assert!(!should_proactive_sync(false, true, 1000, Some(0), 60));
    assert!(!should_proactive_sync(false, true, 10_000, Some(0), 60));
}

#[test]
fn proactive_sync_fires_on_reconnect_regardless_of_last_sync() {
    // offline→online transition always triggers sync to refresh gossip immediately; previous last_sync timestamp is irrelevant.
    assert!(should_proactive_sync(true, false, 1000, None, 60));
    assert!(should_proactive_sync(true, false, 1000, Some(999), 60));
    assert!(should_proactive_sync(true, false, 1000, Some(0), 60));
}

#[test]
fn proactive_sync_fires_when_last_sync_unknown_for_steady_online_peer() {
    // First iteration after a state where peer was already online but we have no record of having synced — must sync to populate the map.
    assert!(should_proactive_sync(true, true, 1000, None, 60));
}

#[test]
fn proactive_sync_fires_when_interval_elapsed() {
    // Peer steady online and last_sync ≥ interval ago → sync to refresh gossip-discovered peers' last_seen before TTL eviction.
    assert!(should_proactive_sync(true, true, 1060, Some(1000), 60));
    assert!(should_proactive_sync(true, true, 1500, Some(1000), 60));
}

#[test]
fn proactive_sync_skips_when_recently_synced() {
    // Peer steady online and last_sync < interval ago → skip to avoid hammering paired peers. Network traffic stays bounded.
    assert!(!should_proactive_sync(true, true, 1059, Some(1000), 60));
    assert!(!should_proactive_sync(true, true, 1001, Some(1000), 60));
    assert!(!should_proactive_sync(true, true, 1000, Some(1000), 60));
}

#[test]
fn proactive_sync_handles_clock_skew_via_saturating_sub() {
    // behavior: don't blast sync requests after clock skew.
    assert!(!should_proactive_sync(true, true, 500, Some(1000), 60));
}
