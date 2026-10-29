use super::{tailscale, TransportState, TransportStatus};
use std::net::IpAddr;

mod macos;
#[cfg(target_os = "macos")]
pub(super) use macos::detect;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    LocalApi,
    BundledCli,
    Interfaces,
}

const MACOS_PROBES: [Probe; 3] = [Probe::LocalApi, Probe::BundledCli, Probe::Interfaces];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureKind {
    Missing,
    Unavailable,
    Permission,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Failure {
    kind: FailureKind,
    detail: String,
}

impl Failure {
    fn io(path: &str, error: std::io::Error) -> Self {
        if let Some(exit) = error
            .get_ref()
            .and_then(|e| e.downcast_ref::<codeterm_util::process_util::CaptureExit>())
        {
            let stderr = exit.stderr.to_ascii_lowercase();
            return Self {
                kind: if stderr.contains("operation not permitted")
                    || stderr.contains("permission denied")
                {
                    FailureKind::Permission
                } else {
                    FailureKind::Unavailable
                },
                detail: format!("{path}: {exit}"),
            };
        }
        let kind = match error.kind() {
            std::io::ErrorKind::PermissionDenied => FailureKind::Permission,
            std::io::ErrorKind::NotFound => FailureKind::Missing,
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotConnected => {
                FailureKind::Unavailable
            }
            _ => FailureKind::Other,
        };
        Self {
            kind,
            detail: format!("{path}: {error} (errno {:?})", error.raw_os_error()),
        }
    }

    fn http(path: &str, status: u16) -> Self {
        Self {
            kind: if matches!(status, 401 | 403) {
                FailureKind::Permission
            } else {
                FailureKind::Other
            },
            detail: format!("{path}: HTTP {status}"),
        }
    }
}

fn unavailable_state(installed: bool, failures: &[Failure]) -> TransportState {
    if failures.iter().any(|f| f.kind == FailureKind::Permission) {
        TransportState::PermissionNeeded
    } else if failures.iter().any(|f| f.kind == FailureKind::Other) {
        TransportState::Unknown
    } else if installed {
        TransportState::NotRunning
    } else {
        TransportState::NotInstalled
    }
}

fn detect_with(
    installed: bool,
    mut probe: impl FnMut(Probe, &mut Vec<Failure>) -> Option<serde_json::Value>,
) -> (TransportStatus, Vec<Failure>) {
    let mut failures = Vec::new();
    let mut partial = None;
    for step in MACOS_PROBES {
        if let Some(value) = probe(step, &mut failures) {
            let status = tailscale::project(true, Some(&value));
            if status.state != TransportState::Connected || !status.addresses.is_empty() {
                return (status, failures);
            }
            partial = Some(value);
        }
    }
    let mut status = tailscale::project(installed || partial.is_some(), partial.as_ref());
    status.state = unavailable_state(installed || partial.is_some(), &failures);
    (status, failures)
}

fn tailnet_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let bytes = ip.octets();
            bytes[0] == 100 && (64..=127).contains(&bytes[1])
        }
        IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

fn interface_address(name: &str, ip: IpAddr) -> bool {
    (name.starts_with("utun") || name.to_ascii_lowercase().starts_with("tailscale"))
        && tailnet_address(ip)
}

fn address_status(addresses: Vec<IpAddr>) -> Option<serde_json::Value> {
    (!addresses.is_empty()).then(
        || serde_json::json!({"BackendState": "Running", "Self": {"TailscaleIPs": addresses}}),
    )
}
