use super::*;
use serde_json::json;

// Tailscale 1.102 refuses a LocalAPI request whose Host omits the listener port.
#[cfg(target_os = "macos")]
#[test]
fn localapi_request_names_the_listener_port_in_host() {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut connection, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(connection.try_clone().unwrap());
        let mut head = String::new();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        connection
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
            )
            .unwrap();
        head
    });
    assert_eq!(
        super::macos_localapi::localapi_get(port, "fixture-proof", "/localapi/v0/status"),
        Some(json!({"ok": true}))
    );
    assert!(server
        .join()
        .unwrap()
        .contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")));
}

fn serve_json(host_port: &str, https_port: &str, proxy: &str) -> serde_json::Value {
    json!({
        "TCP": { https_port: { "HTTPS": true } },
        "Web": { host_port: { "Handlers": { "/": { "Proxy": proxy } } } }
    })
}

fn multi_serve_json() -> serde_json::Value {
    json!({
        "TCP": {
            "443": { "HTTPS": true },
            "8443": { "HTTPS": true },
            "10443": { "HTTPS": true }
        },
        "Web": {
            "host.ts.net:443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:7685" } } },
            "host.ts.net:8443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:7686" } } },
            "host.ts.net:10443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:9000" } } }
        }
    })
}

#[test]
fn target_none_when_no_serve() {
    assert_eq!(parse_serve_target(&json!({}), 7685), None);
}

#[test]
fn url_https_443_targeting_our_port() {
    let v = serve_json("host.ts.net:443", "443", "http://127.0.0.1:7685");
    let (host, port) = parse_serve_target(&v, 7685).unwrap();
    assert_eq!(format_serve_url(&host, port), "https://host.ts.net");
}

#[test]
fn url_https_non_443_keeps_port_suffix() {
    let v = serve_json("host.ts.net:7600", "7600", "http://127.0.0.1:7685");
    let (host, port) = parse_serve_target(&v, 7685).unwrap();
    assert_eq!(format_serve_url(&host, port), "https://host.ts.net:7600");
}

#[test]
fn target_none_when_proxy_targets_different_port() {
    // Serve points at the dev port (7686) but we are the prod daemon (7685):
    // advertising the HTTPS URL here would hand the user a 502 link.
    let v = serve_json("host.ts.net:443", "443", "http://127.0.0.1:7686");
    assert_eq!(parse_serve_target(&v, 7685), None);
}

#[test]
fn target_none_when_https_not_terminated() {
    let v = json!({
        "TCP": { "443": { "HTTPS": false } },
        "Web": { "host.ts.net:443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:7685" } } } }
    });
    assert_eq!(parse_serve_target(&v, 7685), None);
}

#[test]
fn proxy_target_matches_loopback_variants() {
    assert!(proxy_targets_local_port("http://127.0.0.1:7685", 7685));
    assert!(proxy_targets_local_port("http://localhost:7685", 7685));
    assert!(!proxy_targets_local_port("http://127.0.0.1:7686", 7685));
    assert!(!proxy_targets_local_port("http://10.0.0.5:7685", 7685));
    assert!(!proxy_targets_local_port("garbage", 7685));
}

#[test]
fn self_dns_from_serve_config_web_key() {
    let v = serve_json("host.ts.net:443", "443", "http://127.0.0.1:7685");
    assert_eq!(serve_config_self_dns(&v).as_deref(), Some("host.ts.net"));
    assert_eq!(serve_config_self_dns(&json!({})), None);
    // Non-tailnet host keys are ignored.
    let bad = serve_json("localhost:8080", "8080", "http://127.0.0.1:7685");
    assert_eq!(serve_config_self_dns(&bad), None);
}

#[test]
fn collect_routes_marks_owned_and_conflicts_without_dropping_any() {
    let routes = collect_routes(&multi_serve_json(), 7686);
    assert_eq!(routes.len(), 3);
    assert_eq!(routes[0].https_port, 443);
    assert_eq!(routes[0].ownership, ServeRouteOwnership::Conflict);
    assert_eq!(routes[1].https_port, 8443);
    assert_eq!(routes[1].ownership, ServeRouteOwnership::Owned);
    assert_eq!(routes[2].https_port, 10443);
    assert_eq!(routes[2].ownership, ServeRouteOwnership::Conflict);
}

#[test]
fn choose_free_https_port_preserves_existing_mappings() {
    let routes = collect_routes(&multi_serve_json(), 7685);
    assert_eq!(choose_free_https_port(&routes, 443, 7685), Some(11443));
}

#[test]
fn choose_free_https_port_refuses_own_and_sibling_api_ports() {
    let routes = Vec::new();
    assert_eq!(choose_free_https_port(&routes, 7686, 7685), Some(8443));
    assert_eq!(choose_free_https_port(&routes, 7685, 7685), Some(8443));
}

#[test]
fn parse_tailscale_versions_detects_client_daemon_skew() {
    let (cli, daemon) =
        parse_tailscale_versions("1.82.5\n  tailscale commit: abc\n  other commit: def\n1.80.3\n");
    assert_eq!(cli.as_deref(), Some("1.82.5"));
    assert_eq!(daemon.as_deref(), Some("1.80.3"));
    assert_ne!(cli, daemon);
}

#[test]
fn parse_tailscale_versions_rejects_gui_errors_and_numeric_metadata() {
    for raw in [
        "The Tailscale GUI failed to start: The operation couldn't be completed. (Tailscale.CLIError error 3.)\n",
        "3.\n",
        "error 1.98.9\n",
        "tailscale commit: 1234567\nother commit: 9876543\n",
        "go version: go1.26.5\n",
    ] {
        assert_eq!(parse_tailscale_versions(raw), (None, None));
    }
}

#[test]
fn parse_tailscale_versions_accepts_release_and_development_versions() {
    assert_eq!(
        parse_tailscale_versions("1.98.9\n  tailscale commit: 4fb758c\n  go version: go1.26.5\n"),
        (Some("1.98.9".into()), None)
    );
    assert_eq!(
        parse_tailscale_versions("1.99.0-dev+abcdef\n"),
        (Some("1.99.0-dev+abcdef".into()), None)
    );
}

#[test]
fn operator_bootstrap_error_is_explicit() {
    assert!(is_operator_bootstrap_error(
        "Access denied: user is not allowed to modify tailscale serve config"
    ));
    assert!(is_operator_bootstrap_error(
        "set --operator=rollacode before using this command"
    ));
    assert!(!is_operator_bootstrap_error("connection refused"));
}

#[test]
fn sibling_ports_pair_up() {
    assert_eq!(sibling_codeterm_port(7685), Some(7686));
    assert_eq!(sibling_codeterm_port(7686), Some(7685));
    assert_eq!(sibling_codeterm_port(9000), None);
}

#[test]
fn clobber_guard_rejects_own_and_sibling_ports() {
    // Public port == our own API port.
    assert!(would_clobber_codeterm_port(7685, 7685));
    // Public port == sibling Domios API port (the #150 footgun).
    assert!(would_clobber_codeterm_port(7685, 7686));
    assert!(would_clobber_codeterm_port(7686, 7685));
    // 443 / 8443 against a Domios api_port are fine.
    assert!(!would_clobber_codeterm_port(7685, 443));
    assert!(!would_clobber_codeterm_port(7685, 8443));
    // Non-sibling custom port: only self-collision is rejected.
    assert!(would_clobber_codeterm_port(9000, 9000));
    assert!(!would_clobber_codeterm_port(9000, 7686));
}
