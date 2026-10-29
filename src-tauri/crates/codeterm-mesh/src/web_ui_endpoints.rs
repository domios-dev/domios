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
    collect_verified_with(tailscale_ip, local_ip, api_port, https_url, |endpoint| {
        crate::serve_setup::endpoint_matches_build(&endpoint_base_url(endpoint), expected_build)
    })
}

fn collect_verified_with(
    tailscale_ip: Option<&str>,
    local_ip: Option<&str>,
    api_port: u16,
    https_url: Option<&str>,
    verify: impl FnMut(&WebUiEndpoint) -> bool,
) -> Option<WebUiAnnouncement> {
    let announcement = collect(tailscale_ip, local_ip, api_port, https_url)?;
    normalize(WebUiAnnouncement {
        protocol_version: announcement.protocol_version,
        endpoints: announcement.endpoints.into_iter().filter(verify).collect(),
    })
}

pub fn local_http_listener(
    announcement: Option<&WebUiAnnouncement>,
    listeners: &[std::net::SocketAddr],
    healthy: bool,
    port: u16,
) -> Option<std::net::SocketAddr> {
    if !healthy {
        return None;
    }
    announcement
        .into_iter()
        .flat_map(|a| &a.endpoints)
        .find_map(|endpoint| match endpoint {
            WebUiEndpoint::TailscaleHttp { ip, port } | WebUiEndpoint::LanHttp { ip, port } => {
                Some(std::net::SocketAddr::new(*ip, *port))
            }
            _ => None,
        })
        .or_else(|| {
            listeners
                .iter()
                .find(|a| a.port() == port && a.ip().is_loopback())
                .copied()
        })
}

pub fn normalize(announcement: WebUiAnnouncement) -> Option<WebUiAnnouncement> {
    if !types::is_supported_relay_protocol(announcement.protocol_version) {
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
        protocol_version: announcement.protocol_version,
        endpoints,
    })
}

pub fn preferred_https_url(announcement: Option<&WebUiAnnouncement>) -> Option<String> {
    announcement?
        .endpoints
        .iter()
        .find_map(|endpoint| match endpoint {
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
            crate::transport::peer_http_origin(&ip.to_string(), *port)
        }
    }
}

#[cfg(test)]
mod tests;
