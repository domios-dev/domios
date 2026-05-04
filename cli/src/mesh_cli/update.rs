//! Two-step `update --apply` over mesh: force-check (populates the peer's
//! `last_check` cache), then install if an update is available. The install
//! endpoint requires a prior check call.

use crate::data_dir_name;
use crate::UpdateAction;

use super::relay::{relay_call, resolve_peer_url};

/// Resolve update flag shortcuts (--check, --apply, --channel) to a concrete UpdateAction.
/// Same logic as the local CLI dispatch in main.rs.
pub(crate) fn resolve_update_action(
    action: Option<&UpdateAction>,
    check: bool,
    apply: bool,
    channel: Option<String>,
    manifest_url: Option<String>,
) -> UpdateAction {
    if let Some(ch) = channel {
        UpdateAction::Channel {
            channel: if ch.is_empty() { None } else { Some(ch) },
        }
    } else if apply {
        UpdateAction::Install {
            auto: true,
            manifest_url,
            from_file: None,
            continue_prompt: None,
            continue_pane: None,
        }
    } else if check {
        UpdateAction::Check { force: false }
    } else {
        match action {
            Some(UpdateAction::Check { force }) => UpdateAction::Check { force: *force },
            Some(UpdateAction::Install { auto, manifest_url, from_file, continue_prompt, continue_pane }) => UpdateAction::Install {
                auto: *auto,
                manifest_url: manifest_url.clone(),
                from_file: from_file.clone(),
                continue_prompt: continue_prompt.clone(),
                continue_pane: continue_pane.clone(),
            },
            Some(UpdateAction::Channel { channel }) => UpdateAction::Channel { channel: channel.clone() },
            None => UpdateAction::Check { force: false },
        }
    }
}

pub(crate) fn run_update_apply_via_mesh(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    target: &str,
    manifest_url: Option<&str>,
    json_mode: bool,
) {
    let Some(peer_url) = resolve_peer_url(http, base, token, target) else {
        eprintln!("Peer '{}' not found", target);
        std::process::exit(1);
    };

    let signed = match crate::read_mesh_signed_token() {
        Some(t) => t,
        None => {
            eprintln!(
                "Error: cannot sign mesh request — no identity at {:?}.",
                dirs::home_dir().unwrap_or_default().join(data_dir_name()).join("mesh/identity.json")
            );
            std::process::exit(1);
        }
    };

    // Step 1: force-check on peer.
    let check_resp = relay_call(http, &peer_url, &signed, "GET", "/api/update/check?force=true", None);
    let check_data = match check_resp {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[{}] check failed: {}", target, e);
            std::process::exit(1);
        }
    };
    let status = check_data.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
    if status == 404 {
        eprintln!(
            "Peer '{}' is running an older CodeTerm without remote-update support.\n\
             Bootstrap manually one last time, then this command will work going forward.",
            target
        );
        std::process::exit(2);
    }
    if !check_data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let err = check_data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[{}] check failed: {} (HTTP {})", target, err, status);
        std::process::exit(1);
    }
    let info = check_data.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let current = info.get("current_version").and_then(|v| v.as_str()).unwrap_or("?");
    let latest = info.get("latest_version").and_then(|v| v.as_str()).unwrap_or("?");
    let available = info.get("update_available").and_then(|v| v.as_bool()).unwrap_or(false);

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "step": "check",
            "peer": target,
            "data": info,
        })).unwrap_or_default());
    } else {
        println!(
            "[{}] checking…  current v{}, latest v{} — {}",
            target, current, latest,
            if available { "update available" } else { "up to date" }
        );
    }

    if !available {
        return;
    }

    // Step 2: install on peer.
    let mut install_path = String::from("/api/update/install?method=auto");
    if let Some(m) = manifest_url {
        install_path.push_str(&format!("&manifest_url={}", urlencoding::encode(m)));
    }
    let install_resp = relay_call(http, &peer_url, &signed, "POST", &install_path, None);
    let install_data = match install_resp {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[{}] install relay failed: {}", target, e);
            std::process::exit(1);
        }
    };
    let status = install_data.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
    if status == 403 {
        let err = install_data.get("data").and_then(|d| d.get("error")).and_then(|v| v.as_str())
            .or_else(|| install_data.get("error").and_then(|v| v.as_str()))
            .unwrap_or("forbidden");
        eprintln!("[{}] install rejected: {}", target, err);
        std::process::exit(3);
    }
    if status == 404 {
        eprintln!(
            "Peer '{}' is running an older CodeTerm without remote-update support.\n\
             Bootstrap manually one last time, then this command will work going forward.",
            target
        );
        std::process::exit(2);
    }
    if !install_data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let err = install_data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
        eprintln!("[{}] install failed: {} (HTTP {})", target, err, status);
        std::process::exit(1);
    }
    let inner = install_data.get("data").cloned().unwrap_or(serde_json::Value::Null);
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "step": "install",
            "peer": target,
            "data": inner,
        })).unwrap_or_default());
    } else {
        let s = inner.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let msg = inner.get("message").and_then(|v| v.as_str()).unwrap_or("");
        println!("[{}] applying…  status={} {}", target, s, msg);
    }
}
