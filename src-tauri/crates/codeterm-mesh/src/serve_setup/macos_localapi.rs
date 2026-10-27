#[cfg(target_os = "macos")]
use super::PROBE_TIMEOUT;

#[derive(PartialEq, Eq)]
struct Credentials {
    port: u16,
    proof: String,
}

fn proof_candidates(
    app_store_files: &[String],
    system_port: Option<&str>,
    system_proof: Option<&str>,
) -> Vec<Credentials> {
    let mut credentials: Vec<_> = app_store_files
        .iter()
        .filter_map(|name| {
            let (port, proof) = name.strip_prefix("sameuserproof-")?.split_once('-')?;
            parse_credentials(port, proof)
        })
        .collect();
    if let Some(candidate) = system_port
        .zip(system_proof)
        .and_then(|(port, proof)| parse_credentials(port, proof.trim()))
    {
        credentials.push(candidate);
    }
    credentials
}

fn parse_credentials(port: &str, proof: &str) -> Option<Credentials> {
    if port.is_empty()
        || !port.bytes().all(|byte| byte.is_ascii_digit())
        || proof.is_empty()
        || !proof.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    Some(Credentials {
        port,
        proof: proof.to_string(),
    })
}

#[cfg(target_os = "macos")]
pub(super) fn macos_localapi(path: &str) -> Option<serde_json::Value> {
    if !matches!(path, "/localapi/v0/status" | "/localapi/v0/serve-config") {
        return None;
    }
    localapi_via_proof_file(path).or_else(|| {
        let home = dirs::home_dir()?;
        let out = codeterm_util::process_util::disclaimed_capture(
            "/bin/sh",
            &[
                "-c",
                include_str!("macos_localapi.sh"),
                "domios-tailscale-probe",
                home.to_str()?,
                path,
            ],
        )?;
        serde_json::from_str(&out).ok()
    })
}

#[cfg(target_os = "macos")]
fn localapi_via_proof_file(path: &str) -> Option<serde_json::Value> {
    let app_store_files: Vec<_> = dirs::home_dir()
        .and_then(|home| std::fs::read_dir(home.join("Library/Group Containers")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|dir| {
            dir.file_name()
                .to_string_lossy()
                .ends_with("group.io.tailscale.ipn.macos")
        })
        .filter_map(|dir| std::fs::read_dir(dir.path()).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    // The system extension publishes its proof in /Library/Tailscale, outside its Group Container.
    let system_root = std::path::Path::new("/Library/Tailscale");
    let system_port = std::fs::read_link(system_root.join("ipnport"))
        .ok()
        .and_then(|port| port.to_str().map(String::from));
    let system_proof = system_port
        .as_deref()
        .filter(|port| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|port| {
            std::fs::read_to_string(system_root.join(format!("sameuserproof-{port}"))).ok()
        });
    proof_candidates(
        &app_store_files,
        system_port.as_deref(),
        system_proof.as_deref(),
    )
    .into_iter()
    .find_map(|credentials| localapi_get(credentials.port, &credentials.proof, path))
}

/// Minimal raw-HTTP GET against the loopback LocalAPI (avoids a reqwest blocking
/// client panicking when called from inside the async mesh loop).
#[cfg(target_os = "macos")]
pub(super) fn localapi_get(port: u16, proof: &str, path: &str) -> Option<serde_json::Value> {
    use base64::Engine;
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
    let cred = base64::engine::general_purpose::STANDARD.encode(format!(":{proof}"));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Basic {cred}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    // Slice the JSON object out of the body — robust to chunked framing.
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    serde_json::from_str(body.get(start..=end)?).ok()
}

#[cfg(test)]
mod tests;
