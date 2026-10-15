use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use url::{Host, Url};

pub fn loopback_origin(address: SocketAddr) -> Result<String, String> {
    let address = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), address.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv6Addr::LOCALHOST.into(), address.port())
        }
        ip if ip.is_loopback() => address,
        _ => return Err("Desktop requires a loopback daemon listener".into()),
    };
    Ok(format!("http://{address}"))
}

pub fn development_frontend(debug: bool, dev_url: Option<&Url>) -> Result<Option<Url>, String> {
    if !debug {
        return Ok(None);
    }
    let Some(url) = dev_url else {
        return Ok(None);
    };
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(host)) => host == "localhost",
        None => false,
    };
    if !loopback
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Development frontend must use a loopback HTTP origin".into());
    }
    Ok(Some(url.clone()))
}

pub fn fallback_origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

pub fn declared_frontend_origins(
    config: &crate::config::schema::ApiConfig,
    dev_url: Option<&Url>,
) -> Vec<String> {
    let mut origins = vec![fallback_origin().to_string()];
    if let Ok(origin) = crate::config::canonical_api_bind_addr(config).and_then(loopback_origin) {
        origins.push(origin);
    }
    if let Ok(Some(url)) = development_frontend(cfg!(debug_assertions), dev_url) {
        origins.push(url.origin().ascii_serialization());
    }
    origins
}
