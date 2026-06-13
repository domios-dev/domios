//! `codeterm web` — web-mirror helpers. `web link` assembles a ready-to-
//! open URL for THIS machine's /web with the API token embedded so the
//! General Agent can hand the user a one-click link.

use crate::WebAction;

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
    // The link must embed the STABLE api.token from config.toml — never the signed mesh token read_api_config prefers (those expire in ~60s and
    // are nonce-protected against replay, i.e. dead on first reuse).
    let Some(token) = config_api_token().filter(|t| !t.is_empty()) else {
        eprintln!("Error: no api.token in config — web access needs one (Settings → Network).");
        std::process::exit(1);
    };

    let port = port_flag.unwrap_or_else(|| {
        base.rsplit(':')
            .next()
            .and_then(|p| p.trim_end_matches('/').parse().ok())
            .unwrap_or(7685)
    });

    // Host priority: explicit flag > Tailscale (stable across networks) > LAN > loopback. Mesh status is best-effort — loopback still works.
    let host = host_flag.unwrap_or_else(|| {
        let http = crate::build_http(&Some(token.clone())).ok();
        let status = http.and_then(|h| {
            h.get(format!("{}/api/mesh/status", base))
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .ok()?
                .json::<serde_json::Value>()
                .ok()
        });
        status
            .as_ref()
            .and_then(|s| {
                s.get("tailscale_ip")
                    .and_then(|v| v.as_str())
                    .or_else(|| s.get("local_ip").and_then(|v| v.as_str()))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "127.0.0.1".to_string())
    });

    // Fragment is the secure default: it never leaves the browser, so the token can't land in the daemon access log, proxies, or Referer headers.
    // --query opts into the more typeable (but loggable) form.
    let url = if query {
        format!("http://{}:{}/web/?token={}", host, port, token)
    } else {
        format!("http://{}:{}/web/#token={}", host, port, token)
    };

    if json_mode {
        println!(
            "{}",
            serde_json::json!({ "url": url, "host": host, "port": port })
        );
    } else {
        println!("{}", url);
    }
}

/// Build a /web link for a paired mesh peer: resolve the peer name to its
/// instance id, then let the peer mint its own web token. The daemon's
/// peer-web-token route already returns a `host` that prefers the peer's
/// HTTPS tailscale-serve FQDN over a mixed-content http://IP front, so the
/// resulting link works from an https-origin browser.
fn link_peer(json_mode: bool, base: &str, peer_name: &str, query: bool) {
    let Some(token) = config_api_token().filter(|t| !t.is_empty()) else {
        eprintln!("Error: no api.token in config — web access needs one (Settings → Network).");
        std::process::exit(1);
    };
    let Ok(http) = crate::build_http(&Some(token.clone())) else {
        eprintln!("Error: failed to build HTTP client.");
        std::process::exit(1);
    };

    let peers = http
        .get(format!("{}/api/mesh/peers", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok());
    let Some(peers) = peers else {
        eprintln!("Error: could not reach the daemon to list mesh peers.");
        std::process::exit(1);
    };
    // Match on instance name (case-insensitive) or an instance-id prefix.
    let id = peers
        .get("paired")
        .and_then(|p| p.as_array())
        .and_then(|arr| {
            arr.iter().find(|p| {
                let name = p.get("instance_name").and_then(|v| v.as_str()).unwrap_or("");
                let pid = p.get("instance_id").and_then(|v| v.as_str()).unwrap_or("");
                name.eq_ignore_ascii_case(peer_name) || pid.starts_with(peer_name)
            })
        })
        .and_then(|p| p.get("instance_id").and_then(|v| v.as_str()))
        .map(str::to_string);
    let Some(id) = id else {
        eprintln!(
            "Error: no paired peer matching '{}'. Run `codeterm mesh peers` to list.",
            peer_name
        );
        std::process::exit(1);
    };

    let resp = http
        .get(format!("{}/api/mesh/peers/{}/web-token", base, id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .filter(|r| r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    let Some(resp) = resp else {
        eprintln!(
            "Error: could not mint a web token for peer '{}' (peer offline or web access disabled).",
            peer_name
        );
        std::process::exit(1);
    };
    let host = resp.get("host").and_then(|v| v.as_str()).unwrap_or_default();
    let peer_token = resp.get("token").and_then(|v| v.as_str()).unwrap_or_default();
    if host.is_empty() || peer_token.is_empty() {
        eprintln!("Error: peer returned an incomplete web link.");
        std::process::exit(1);
    }

    // `host` already carries the scheme (https://fqdn or http://ip:port).
    let url = if query {
        format!("{}/web/?token={}", host, peer_token)
    } else {
        format!("{}/web/#token={}", host, peer_token)
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
