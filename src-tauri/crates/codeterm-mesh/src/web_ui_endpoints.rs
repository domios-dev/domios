use std::net::IpAddr;

use crate::types::{self, WebUiAnnouncement, WebUiEndpoint};

pub fn collect(
    tailscale_ip: Option<&str>,
    local_ip: Option<&str>,
    api_port: u16,
    https_url: Option<&str>,
) -> Option<WebUiAnnouncement> {
    let mut endpoints = Vec::new();
    if let Some(endpoint) = https_url.and_then(parse_https_endpoint) {
        endpoints.push(endpoint);
    }
    if let Some(ip) = tailscale_ip.and_then(parse_tailscale_ip) {
        endpoints.push(WebUiEndpoint::TailscaleHttp { ip, port: api_port });
    }
    if let Some(ip) = local_ip.and_then(parse_lan_ip) {
        endpoints.push(WebUiEndpoint::LanHttp { ip, port: api_port });
    }
    normalize(WebUiAnnouncement {
        protocol_version: types::RELAY_PROTOCOL_VERSION,
        endpoints,
    })
}

pub fn collect_verified(
    tailscale_ip: Option<&str>,
    local_ip: Option<&str>,
    api_port: u16,
    https_url: Option<&str>,
    expected_build: &str,
) -> Option<WebUiAnnouncement> {
    let announcement = collect(tailscale_ip, local_ip, api_port, https_url)?;
    normalize(WebUiAnnouncement {
        protocol_version: announcement.protocol_version,
        endpoints: announcement
            .endpoints
            .into_iter()
            .filter(|endpoint| {
                crate::serve_setup::endpoint_matches_build(
                    &endpoint_base_url(endpoint),
                    expected_build,
                )
            })
            .collect(),
    })
}

pub fn normalize(announcement: WebUiAnnouncement) -> Option<WebUiAnnouncement> {
    if announcement.protocol_version != types::RELAY_PROTOCOL_VERSION {
        return None;
    }
    let mut endpoints = announcement
        .endpoints
        .into_iter()
        .filter(is_valid)
        .collect::<Vec<_>>();
    endpoints.sort_by_key(rank);
    endpoints.dedup();
    (!endpoints.is_empty()).then_some(WebUiAnnouncement {
        protocol_version: types::RELAY_PROTOCOL_VERSION,
        endpoints,
    })
}

pub fn preferred_https_url(announcement: Option<&WebUiAnnouncement>) -> Option<String> {
    announcement?.endpoints.iter().find_map(|endpoint| match endpoint {
        WebUiEndpoint::TailscaleHttps { host, port } => Some(if *port == 443 {
            format!("https://{host}")
        } else {
            format!("https://{host}:{port}")
        }),
        _ => None,
    })
}

fn parse_https_endpoint(url: &str) -> Option<WebUiEndpoint> {
    let authority = url.strip_prefix("https://")?.trim_end_matches('/');
    if authority.contains(['/', '?', '#', '@']) {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (authority, 443),
    };
    let endpoint = WebUiEndpoint::TailscaleHttps {
        host: host.to_ascii_lowercase(),
        port,
    };
    is_valid(&endpoint).then_some(endpoint)
}

fn parse_tailscale_ip(value: &str) -> Option<IpAddr> {
    let ip = value.parse().ok()?;
    types::is_tailscale_ip(value).then_some(ip)
}

fn parse_lan_ip(value: &str) -> Option<IpAddr> {
    let ip: IpAddr = value.parse().ok()?;
    (types::is_lan_private_ip(value) && !ip.is_loopback()).then_some(ip)
}

fn is_valid(endpoint: &WebUiEndpoint) -> bool {
    match endpoint {
        WebUiEndpoint::TailscaleHttps { host, port } => {
            *port != 0
                && host.len() <= 253
                && host.ends_with(".ts.net")
                && host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        }
        WebUiEndpoint::TailscaleHttp { ip, port } => {
            *port != 0 && types::is_tailscale_ip(&ip.to_string())
        }
        WebUiEndpoint::LanHttp { ip, port } => {
            *port != 0 && !ip.is_loopback() && types::is_lan_private_ip(&ip.to_string())
        }
    }
}

fn rank(endpoint: &WebUiEndpoint) -> u8 {
    match endpoint {
        WebUiEndpoint::TailscaleHttps { .. } => 0,
        WebUiEndpoint::TailscaleHttp { .. } => 1,
        WebUiEndpoint::LanHttp { .. } => 2,
    }
}

fn endpoint_base_url(endpoint: &WebUiEndpoint) -> String {
    match endpoint {
        WebUiEndpoint::TailscaleHttps { host, port } if *port == 443 => {
            format!("https://{host}")
        }
        WebUiEndpoint::TailscaleHttps { host, port } => format!("https://{host}:{port}"),
        WebUiEndpoint::TailscaleHttp { ip, port } | WebUiEndpoint::LanHttp { ip, port } => {
            format!("http://{ip}:{port}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_ranks_https_then_tailscale_then_lan() {
        let announcement = collect(
            Some("100.64.0.8"),
            Some("192.168.1.8"),
            7685,
            Some("https://host.tailnet.ts.net"),
        )
        .expect("endpoints");
        assert!(matches!(announcement.endpoints[0], WebUiEndpoint::TailscaleHttps { .. }));
        assert!(matches!(announcement.endpoints[1], WebUiEndpoint::TailscaleHttp { .. }));
        assert!(matches!(announcement.endpoints[2], WebUiEndpoint::LanHttp { .. }));
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
}
