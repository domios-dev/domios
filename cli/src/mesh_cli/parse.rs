//! Small input parsers shared across mesh subcommands.

/// Split an "ip[:port]" string into (ip, optional port). Best-effort —
/// IPv6 with brackets is supported, plain IPv4 with optional ":port" is
/// the typical case.
pub fn parse_ip_port(input: &str) -> (String, Option<u16>) {
    let s = input.trim();
    // [ipv6]:port form
    if let Some(rest) = s.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let ip = rest[..end].to_string();
            let tail = &rest[end + 1..];
            let port = tail.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
            return (ip, port);
        }
    }
    // ipv4[:port] — split on the LAST colon to avoid breaking bare ipv6
    if let Some(idx) = s.rfind(':') {
        let (ip, port_str) = s.split_at(idx);
        let port_str = &port_str[1..];
        if !ip.contains(':') {
            // Plain ipv4 with port
            if let Ok(p) = port_str.parse::<u16>() {
                return (ip.to_string(), Some(p));
            }
        }
    }
    (s.to_string(), None)
}
