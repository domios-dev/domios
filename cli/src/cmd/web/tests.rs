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
