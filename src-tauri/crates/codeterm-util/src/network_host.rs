use std::net::{IpAddr, Ipv6Addr};
use std::str::FromStr;

pub fn normalize_network_host(value: &str) -> Result<String, String> {
    let (host, port) = parse_authority(value)?;
    let host = normalize_host(host)?;
    Ok(format_authority(&host, port))
}

pub fn network_host_allowed(request: &str, allowed: &[String]) -> bool {
    let Ok((request_host, request_port)) = normalized_parts(request) else {
        return false;
    };
    allowed.iter().any(|entry| {
        normalized_parts(entry).is_ok_and(|(host, port)| {
            host == request_host && port.is_none_or(|p| Some(p) == request_port)
        })
    })
}

fn normalized_parts(value: &str) -> Result<(String, Option<u16>), String> {
    let (host, port) = parse_authority(value)?;
    Ok((normalize_host(host)?, port))
}

fn parse_authority(value: &str) -> Result<(&str, Option<u16>), String> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_whitespace) {
        return Err("host must be an exact hostname or IP with an optional port".into());
    }
    if value.contains("*")
        || value.contains("/")
        || value.contains("?")
        || value.contains('#')
        || value.contains('@')
        || value.contains('%')
        || value.contains("://")
    {
        return Err("host must not include a scheme, path, wildcard, or credentials".into());
    }
    if let Some(after_open) = value.strip_prefix('[') {
        let close = after_open
            .find(']')
            .ok_or_else(|| "invalid IPv6 host".to_string())?;
        let host = &after_open[..close];
        Ipv6Addr::from_str(host).map_err(|_| "invalid IPv6 host".to_string())?;
        let suffix = &after_open[close + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            Some(parse_port(
                suffix
                    .strip_prefix(':')
                    .ok_or_else(|| "invalid host port".to_string())?,
            )?)
        };
        return Ok((host, port));
    }
    let mut parts = value.split(':');
    let host = parts.next().unwrap_or_default();
    let port = match (parts.next(), parts.next()) {
        (None, None) => None,
        (Some(port), None) => Some(parse_port(port)?),
        _ => return Err("IPv6 hosts must use brackets".into()),
    };
    Ok((host, port))
}

fn parse_port(value: &str) -> Result<u16, String> {
    let port = value
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| "port must be between 1 and 65535".to_string())?;
    Ok(port)
}

fn normalize_host(host: &str) -> Result<String, String> {
    if let Ok(ip) = IpAddr::from_str(host) {
        return Ok(ip.to_string().to_ascii_lowercase());
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    let valid = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if valid {
        Ok(host.to_ascii_lowercase())
    } else {
        Err("host must be an exact hostname or IP with an optional port".into())
    }
}

fn format_authority(host: &str, port: Option<u16>) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    match port {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_host_grants_normalize_exact_authorities() {
        assert_eq!(
            normalize_network_host("Example.test").unwrap(),
            "example.test"
        );
        assert_eq!(
            normalize_network_host("EXAMPLE.test:443").unwrap(),
            "example.test:443"
        );
        assert_eq!(normalize_network_host("[::1]:7686").unwrap(), "[::1]:7686");
        assert!(normalize_network_host("https://example.test").is_err());
        assert!(normalize_network_host("*.example.test").is_err());
        assert!(normalize_network_host("example.test/path").is_err());
        assert!(normalize_network_host("example.test:bad").is_err());
        assert!(normalize_network_host("::1").is_err());
    }

    #[test]
    fn a_host_grant_is_exact_and_a_port_grant_is_port_scoped() {
        let allowed = vec!["example.test".to_string(), "api.test:8080".to_string()];
        assert!(network_host_allowed("EXAMPLE.test:1234", &allowed));
        assert!(network_host_allowed("api.test:8080", &allowed));
        assert!(!network_host_allowed("api.test:8081", &allowed));
        assert!(!network_host_allowed("sub.example.test", &allowed));
    }
}
