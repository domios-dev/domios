use super::*;

#[test]
fn tailnet_ipv6_peer_clients_use_bracketed_socket_authorities() {
    for ip in ["100.64.1.2", "fd7a:115c:a1e0::1"] {
        let expected = if ip.contains(':') {
            "http://[fd7a:115c:a1e0::1]:7685"
        } else {
            "http://100.64.1.2:7685"
        };
        assert_eq!(
            PeerClient::new_signed(ip, 7685, "signed").base_url,
            expected
        );
        assert_eq!(PeerClient::new_unauthenticated(ip, 7685).base_url, expected);
        assert!(reqwest::Url::parse(expected).is_ok());
    }
}
