//! `codeterm web` — web-mirror helpers. `web link` assembles a ready-to-
//! open URL for THIS machine's browser UI with the API token embedded so the
//! General Agent can hand the user a one-click link.

use crate::WebAction;

#[derive(Debug, Clone, PartialEq)]
struct LocalWebLink {
    url: String,
    host: String,
    port: u16,
}

pub fn handle_web(json_mode: bool, action: WebAction) {
    match action {
        WebAction::Link {
            peer,
            host,
            port,
            query,
        } => link(json_mode, peer, host, port, query),
    }
}

fn link(
    json_mode: bool,
    peer_flag: Option<String>,
    host_flag: Option<String>,
    port_flag: Option<u16>,
    query: bool,
) {
    let (base, _auth) = crate::read_api_config();
    if let Some(peer) = peer_flag {
        link_peer(json_mode, &base, &peer, query);
        return;
    }
    // are nonce-protected against replay, i.e. dead on first reuse).
    let Some(token) = config_api_token().filter(|t| !t.is_empty()) else {
        crate::cmd::print::fail(
            1,
            "no api.token in config — web access needs one (Settings → Network).",
        );
    };

    let explicit_port = port_flag.is_some();
    let port = port_flag.unwrap_or_else(|| {
        base.rsplit(':')
            .next()
            .and_then(|p| p.trim_end_matches('/').parse().ok())
            .unwrap_or(7685)
    });

    let status = fetch_local_network_status(&base, &token);
    let link = build_local_web_link(
        status.as_ref(),
        host_flag.as_deref(),
        port,
        explicit_port,
        &token,
        query,
    );

    if json_mode {
        println!(
            "{}",
            serde_json::json!({ "url": link.url, "host": link.host, "port": link.port })
        );
    } else {
        println!("{}", link.url);
    }
}

fn fetch_local_network_status(base: &str, token: &str) -> Option<serde_json::Value> {
    let http = crate::build_local_api().ok()?;
    http.get(format!("{}/api/network", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .or_else(|| {
            http.get(format!("{}/api/mesh/status", base))
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .ok()
                .and_then(|r| r.json::<serde_json::Value>().ok())
        })
}

fn build_local_web_link(
    status: Option<&serde_json::Value>,
    host_override: Option<&str>,
    port: u16,
    explicit_port: bool,
    token: &str,
    query: bool,
) -> LocalWebLink {
    let explicit_host = host_override.is_some();
    let base = if explicit_host {
        let host = host_override.unwrap_or("127.0.0.1").to_string();
        (format!("http://{}:{}", host, port), host, port)
    } else if !explicit_port {
        if let Some(https) = status
            .and_then(|s| s.get("tailscale_https_url"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            let normalized = https.trim_end_matches('/').to_string();
            let (host, https_port) = origin_host_port(&normalized, 443);
            (normalized, host, https_port)
        } else {
            fallback_http_link(status, port, explicit_port)
        }
    } else {
        fallback_http_link(status, port, explicit_port)
    };
    let separator = if query { "?" } else { "#" };
    LocalWebLink {
        url: format!("{}/{}token={}", base.0, separator, token),
        host: base.1,
        port: base.2,
    }
}

fn fallback_http_link(
    status: Option<&serde_json::Value>,
    port: u16,
    explicit_port: bool,
) -> (String, String, u16) {
    let status_port = if explicit_port {
        port
    } else {
        status
            .and_then(|s| s.get("api_port"))
            .and_then(|v| v.as_u64())
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(port)
    };
    let host = status
        .and_then(|s| {
            s.get("tailscale_ip")
                .and_then(|v| v.as_str())
                .or_else(|| s.get("local_ip").and_then(|v| v.as_str()))
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("127.0.0.1")
        .to_string();
    (
        format!("http://{}:{}", host, status_port),
        host,
        status_port,
    )
}

fn origin_host_port(origin: &str, default_port: u16) -> (String, u16) {
    let authority = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin)
        .split('/')
        .next()
        .unwrap_or(origin);
    if let Some((host, port)) = authority.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    (authority.to_string(), default_port)
}

/// Build a browser UI link for a paired mesh peer: resolve the peer name to its
/// instance id, then let the peer mint its own web token. The daemon's
/// peer-web-token route already returns a `host` that prefers the peer's
/// HTTPS tailscale-serve FQDN over a mixed-content http://IP front, so the
/// resulting link works from an https-origin browser.
fn link_peer(json_mode: bool, base: &str, peer_name: &str, query: bool) {
    let Some(token) = config_api_token().filter(|t| !t.is_empty()) else {
        crate::cmd::print::fail(
            1,
            "no api.token in config — web access needs one (Settings → Network).",
        );
    };
    let Ok(http) = crate::build_local_api() else {
        crate::cmd::print::fail(1, "failed to build HTTP client.");
    };

    let peers = http
        .get(format!("{}/api/mesh/peers", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok());
    let Some(peers) = peers else {
        crate::cmd::print::fail(1, "could not reach the daemon to list mesh peers.");
    };
    // Match on instance name (case-insensitive) or an instance-id prefix.
    let id = peers
        .get("paired")
        .and_then(|p| p.as_array())
        .and_then(|arr| {
            arr.iter().find(|p| {
                let name = p
                    .get("instance_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let pid = p.get("instance_id").and_then(|v| v.as_str()).unwrap_or("");
                name.eq_ignore_ascii_case(peer_name) || pid.starts_with(peer_name)
            })
        })
        .and_then(|p| p.get("instance_id").and_then(|v| v.as_str()))
        .map(str::to_string);
    let Some(id) = id else {
        crate::cmd::print::fail(
            1,
            format!(
                "no paired peer matching '{}'. Run `codeterm mesh peers` to list.",
                peer_name
            ),
        );
    };

    let resp = http
        .get(format!("{}/api/mesh/peers/{}/web-token", base, id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .filter(|r| r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    let Some(resp) = resp else {
        crate::cmd::print::fail(
            1,
            format!(
                "could not mint a web token for peer '{}' (peer offline or web access disabled).",
                peer_name
            ),
        );
    };
    let host = resp
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let peer_token = resp
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if host.is_empty() || peer_token.is_empty() {
        crate::cmd::print::fail(1, "peer returned an incomplete web link.");
    }

    // `host` already carries the scheme (https://fqdn or http://ip:port).
    let url = if query {
        format!("{}/?token={}", host, peer_token)
    } else {
        format!("{}/#token={}", host, peer_token)
    };
    if json_mode {
        println!(
            "{}",
            serde_json::json!({ "url": url, "host": host, "peer": peer_name })
        );
    } else {
        println!("{}", url);
    }
}

/// The persistent `api.token` from config.toml — the credential the web
/// gate stores and replays across sessions.
fn config_api_token() -> Option<String> {
    let path = dirs::home_dir()?
        .join(crate::data_dir_name())
        .join("config.toml");
    let content = std::fs::read_to_string(path).ok()?;
    let table = content.parse::<toml::Table>().ok()?;
    table
        .get("api")?
        .as_table()?
        .get("token")?
        .as_str()
        .map(String::from)
}

#[cfg(test)]
mod tests {
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
}
