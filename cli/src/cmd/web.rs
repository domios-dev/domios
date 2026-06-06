//! `codeterm web` — web-mirror helpers. `web link` assembles a ready-to-
//! open URL for THIS machine's /web with the API token embedded so the
//! General Agent can hand the user a one-click link.

use crate::WebAction;

pub fn handle_web(json_mode: bool, action: WebAction) {
    match action {
        WebAction::Link { host, port, query } => link(json_mode, host, port, query),
    }
}

fn link(json_mode: bool, host_flag: Option<String>, port_flag: Option<u16>, query: bool) {
    let (base, _auth) = crate::read_api_config();
    // The link must embed the STABLE api.token from config.toml — never
    // the signed mesh token read_api_config prefers (those expire in ~60s
    // and are nonce-protected against replay, i.e. dead on first reuse).
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

    // Host priority: explicit flag > Tailscale (stable across networks) >
    // LAN > loopback. Mesh status is best-effort — loopback still works.
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

    // Fragment is the secure default: it never leaves the browser, so the
    // token can't land in the daemon access log, proxies, or Referer
    // headers. --query opts into the more typeable (but loggable) form.
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
