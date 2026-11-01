use super::*;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
const TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn detect() -> TransportStatus {
    let installed = Path::new("/Applications/Tailscale.app").exists()
        || Path::new("/Library/Tailscale").exists()
        || Path::new("/var/run/tailscaled.socket").exists();
    let (status, failures) = detect_with(installed, |probe, failures| match probe {
        Probe::LocalApi => local_api(failures),
        Probe::BundledCli => cli(failures),
        Probe::Interfaces => interfaces(failures),
    });
    static LAST: OnceLock<parking_lot::Mutex<Vec<Failure>>> = OnceLock::new();
    let mut previous = LAST.get_or_init(Default::default).lock();
    if *previous != failures {
        for failure in &failures {
            log::debug!("[mesh/tailscale] {:?}: {}", failure.kind, failure.detail);
        }
        *previous = failures;
    }
    status
}

fn other(detail: impl Into<String>) -> Failure {
    Failure {
        kind: FailureKind::Other,
        detail: detail.into(),
    }
}

fn status_body(path: &str, raw: &[u8]) -> Result<serde_json::Value, Failure> {
    let value: serde_json::Value = serde_json::from_slice(raw)
        .map_err(|e| other(format!("{path}: invalid status JSON: {e}")))?;
    if value["BackendState"].is_string() {
        Ok(value)
    } else {
        Err(other(format!("{path}: missing BackendState")))
    }
}

fn tcp_status(port: u16, token: &str, path: &str) -> Result<serde_json::Value, Failure> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| other(format!("{path}: {e}")))?;
    let response = client
        .get(format!("http://127.0.0.1:{port}/localapi/v0/status"))
        .basic_auth("", Some(token))
        .send()
        .map_err(|e| {
            let mut source = std::error::Error::source(&e);
            while let Some(error) = source {
                if let Some(error) = error.downcast_ref::<std::io::Error>() {
                    return Failure::io(
                        path,
                        error
                            .raw_os_error()
                            .map(std::io::Error::from_raw_os_error)
                            .unwrap_or_else(|| {
                                std::io::Error::new(error.kind(), error.to_string())
                            }),
                    );
                }
                source = error.source();
            }
            other(format!("{path}: {e:?}"))
        })?;
    let code = response.status().as_u16();
    if code != 200 {
        return Err(Failure::http(path, code));
    }
    let mut body = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|e| Failure::io(path, e))?;
    if body.len() > 1024 * 1024 {
        return Err(other(format!("{path}: response exceeds 1 MiB")));
    }
    status_body(path, &body)
}

fn local_api(failures: &mut Vec<Failure>) -> Option<serde_json::Value> {
    let deadline = Instant::now() + Duration::from_secs(6);
    if let Some(home) = dirs::home_dir() {
        let groups = home.join("Library/Group Containers");
        match std::fs::read_dir(&groups) {
            Ok(entries) => {
                for entry in entries.take(128).flatten() {
                    if !entry
                        .file_name()
                        .to_string_lossy()
                        .ends_with("group.io.tailscale.ipn.macos")
                    {
                        continue;
                    }
                    let group = entry.path();
                    match std::fs::read_dir(&group) {
                        Ok(proofs) => {
                            for proof in proofs.take(32).flatten() {
                                if Instant::now() >= deadline {
                                    break;
                                }
                                let name = proof.file_name();
                                let Some((port, token)) = name
                                    .to_str()
                                    .and_then(|n| n.strip_prefix("sameuserproof-"))
                                    .and_then(|n| n.split_once('-'))
                                else {
                                    continue;
                                };
                                let Ok(port) = port.parse::<u16>() else {
                                    continue;
                                };
                                if port == 0
                                    || token.is_empty()
                                    || !token.bytes().all(|b| b.is_ascii_hexdigit())
                                {
                                    continue;
                                }
                                match tcp_status(port, token, &group.to_string_lossy()) {
                                    Ok(value) => return Some(value),
                                    Err(error) => failures.push(error),
                                }
                            }
                        }
                        Err(error) => failures.push(Failure::io(&group.to_string_lossy(), error)),
                    }
                }
            }
            Err(error) => failures.push(Failure::io(&groups.to_string_lossy(), error)),
        }
    }
    let port_path = "/Library/Tailscale/ipnport";
    let standalone = (|| {
        let port = std::fs::read_link(port_path).map_err(|e| Failure::io(port_path, e))?;
        let port = port
            .to_str()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|p| *p != 0)
            .ok_or_else(|| other(format!("{port_path}: invalid port")))?;
        let proof = format!("/Library/Tailscale/sameuserproof-{port}");
        let token = std::fs::read_to_string(&proof).map_err(|e| Failure::io(&proof, e))?;
        let token = token.trim();
        if token.is_empty() || token.len() > 256 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(other(format!("{proof}: invalid proof")));
        }
        tcp_status(port, token, &proof)
    })();
    match standalone {
        Ok(value) => return Some(value),
        Err(error) => failures.push(error),
    }
    for socket in [
        "/var/run/tailscaled.socket",
        "/var/run/tailscale/tailscaled.sock",
    ] {
        if let Err(error) = std::fs::metadata(socket) {
            failures.push(Failure::io(socket, error));
            continue;
        }
        let result = run(
            "/usr/bin/curl",
            &[
                "--disable",
                "--silent",
                "--show-error",
                "--noproxy",
                "*",
                "--max-time",
                "3",
                "--max-filesize",
                "1048576",
                "--unix-socket",
                socket,
                "--write-out",
                "\n%{http_code}",
                "http://local-tailscaled.sock/localapi/v0/status",
            ],
        )
        .and_then(|raw| {
            let (body, status) = raw
                .rsplit_once('\n')
                .ok_or_else(|| other(format!("{socket}: missing HTTP status")))?;
            let code = status
                .trim()
                .parse::<u16>()
                .map_err(|_| other(format!("{socket}: invalid HTTP status")))?;
            if code != 200 {
                return Err(Failure::http(socket, code));
            }
            status_body(socket, body.as_bytes())
        });
        match result {
            Ok(value) => return Some(value),
            Err(mut error) => {
                error.detail = format!("{socket}: {}", error.detail);
                failures.push(error);
            }
        }
    }
    None
}

fn cli(failures: &mut Vec<Failure>) -> Option<serde_json::Value> {
    let mut addresses = Vec::new();
    for family in ["-4", "-6"] {
        match run(CLI, &["ip", family]) {
            Ok(raw) => {
                let found: Vec<IpAddr> = raw
                    .lines()
                    .filter_map(|s| s.trim().parse().ok())
                    .filter(|ip| tailnet_address(*ip) && ip.is_ipv4() == (family == "-4"))
                    .collect();
                if found.is_empty() {
                    failures.push(Failure {
                        kind: FailureKind::Unavailable,
                        detail: format!("{CLI} ip {family}: no tailnet address"),
                    });
                }
                addresses.extend(found);
            }
            Err(mut error) => {
                error.detail = format!("ip {family}: {}", error.detail);
                failures.push(error);
            }
        }
    }
    address_status(addresses)
}

fn interfaces(failures: &mut Vec<Failure>) -> Option<serde_json::Value> {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => {
            let mut addresses: Vec<_> = interfaces
                .into_iter()
                .filter(|i| interface_address(&i.name, i.ip()))
                .map(|i| i.ip())
                .collect();
            addresses.sort_unstable();
            addresses.dedup();
            let status = address_status(addresses);
            if status.is_none() {
                failures.push(Failure {
                    kind: FailureKind::Unavailable,
                    detail: "interfaces utun/tailscale: no tailnet address".into(),
                });
            }
            status
        }
        Err(error) => {
            failures.push(Failure::io("get_if_addrs", error));
            None
        }
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, Failure> {
    codeterm_util::process_util::bounded_capture(program, args, TIMEOUT)
        .map_err(|error| Failure::io(program, error))
}
