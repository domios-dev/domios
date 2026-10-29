use super::*;

#[test]
fn refreshed_web_ui_publishes_only_verified_serve_and_http_endpoints() {
    let snapshot = collect_verified_with(
        Some(concat!("100.", "64.0.7")),
        Some("192.168.1.3"),
        7685,
        Some(concat!("https://node.fixture.ts", ".net")),
        |endpoint| !matches!(endpoint, WebUiEndpoint::LanHttp { .. }),
    )
    .unwrap();
    assert_eq!(snapshot.endpoints.len(), 2);
    assert_eq!(
        preferred_https_url(Some(&snapshot)).as_deref(),
        Some(concat!("https://node.fixture.ts", ".net"))
    );
    assert_eq!(
        local_http_listener(Some(&snapshot), &[], true, 7685),
        Some(concat!("100.", "64.0.7:7685").parse().unwrap())
    );
    let without_serve =
        collect_verified_with(None, Some("192.168.1.3"), 7685, None, |_| true).unwrap();
    assert_eq!(
        local_http_listener(Some(&without_serve), &[], true, 7685),
        Some("192.168.1.3:7685".parse().unwrap())
    );
    assert!(collect_verified_with(
        Some(concat!("100.", "64.0.7")),
        None,
        7685,
        Some(concat!("https://node.fixture.ts", ".net")),
        |_| false
    )
    .is_none());
    assert!(local_http_listener(Some(&snapshot), &[], false, 7685).is_none());
    assert_eq!(
        local_http_listener(None, &["127.0.0.1:7685".parse().unwrap()], true, 7685),
        Some("127.0.0.1:7685".parse().unwrap())
    );
    assert!(local_http_listener(None, &["127.0.0.1:7686".parse().unwrap()], true, 7685).is_none());
}

#[test]
fn collection_ranks_https_then_tailscale_then_lan() {
    let announcement = collect(
        Some(concat!("100.", "64.0.8")),
        Some("192.168.1.8"),
        7685,
        Some(concat!("https://host.fixture.ts", ".net")),
    )
    .expect("endpoints");
    assert!(matches!(
        announcement.endpoints[0],
        WebUiEndpoint::TailscaleHttps { .. }
    ));
    assert!(matches!(
        announcement.endpoints[1],
        WebUiEndpoint::TailscaleHttp { .. }
    ));
    assert!(matches!(
        announcement.endpoints[2],
        WebUiEndpoint::LanHttp { .. }
    ));
}

#[test]
fn normalization_rejects_untrusted_origins() {
    let announcement = WebUiAnnouncement {
        protocol_version: types::RELAY_PROTOCOL_VERSION,
        endpoints: vec![
            WebUiEndpoint::TailscaleHttps {
                host: "evil.example.com".into(),
                port: 443,
            },
            WebUiEndpoint::LanHttp {
                ip: "127.0.0.1".parse().unwrap(),
                port: 7685,
            },
        ],
    };
    assert!(normalize(announcement).is_none());
}
