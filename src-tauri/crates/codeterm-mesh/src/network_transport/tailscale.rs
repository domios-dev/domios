use super::{
    NetworkIdentity, NetworkTransportProvider, SetupAction, TransportState, TransportStatus,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub struct Tailscale;

impl NetworkTransportProvider for Tailscale {
    fn detect(&self) -> TransportStatus {
        let status = crate::discovery::tailscale_status_json();
        let installed = status.is_some() || installed();
        project(installed, status.as_ref())
    }
}

fn installed() -> bool {
    #[cfg(target_os = "macos")]
    if std::path::Path::new("/Applications/Tailscale.app").exists() {
        return true;
    }
    crate::discovery::tailscale_binary().is_some()
}

pub(super) fn project(installed: bool, value: Option<&Value>) -> TransportStatus {
    let backend = value.and_then(|v| v["BackendState"].as_str());
    let state = if !installed {
        TransportState::NotInstalled
    } else {
        match backend {
            Some("Running") => TransportState::Connected,
            Some("Stopped") => TransportState::NotRunning,
            Some("NeedsLogin") => TransportState::SignedOut,
            Some("Starting") => TransportState::Starting,
            None => TransportState::NotRunning,
            _ => TransportState::Unknown,
        }
    };
    let suffix = value
        .and_then(|v| v["CurrentTailnet"]["MagicDNSSuffix"].as_str())
        .map(|s| s.trim_end_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let identity = suffix.map(|suffix| NetworkIdentity {
        kind: "tailscale".into(),
        network_id: format!("{:x}", Sha256::digest(suffix.as_bytes())),
    });
    let network_name = value
        .and_then(|v| v["CurrentTailnet"]["Name"].as_str())
        .map(str::to_owned);
    let user = value.and_then(|v| {
        let id = v["Self"]["UserID"].as_u64()?.to_string();
        v["User"][id]["LoginName"].as_str().map(str::to_owned)
    });
    let addresses = value
        .and_then(|v| v["Self"]["TailscaleIPs"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str()?.parse().ok())
        .collect();
    TransportStatus {
        display_name: "Tailscale".into(),
        state,
        identity,
        network_name,
        user,
        addresses,
        peer_listener: true,
        encrypted: true,
        setup_actions: vec![
            SetupAction {
                kind: "install".into(),
                url: "https://tailscale.com/download".into(),
            },
            SetupAction {
                kind: "sign_in".into(),
                url: "https://login.tailscale.com/".into(),
            },
        ],
    }
}
