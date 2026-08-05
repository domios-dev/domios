//! Two-step `update install --auto` over mesh: force-check (populates the peer's
//! `last_check` cache), then install if an update is available. The install
//! endpoint requires a prior check call.

use crate::UpdateAction;

use super::relay::mesh_relay_invoke_result;

/// Resolve update subcommands and top-level update options to a concrete UpdateAction.
pub(crate) fn resolve_update_action(
    action: Option<&UpdateAction>,
    manifest_url: Option<String>,
    after_update: Option<String>,
) -> UpdateAction {
    match action {
        Some(UpdateAction::Check { force }) => UpdateAction::Check { force: *force },
        Some(UpdateAction::Install {
            auto,
            manifest_url: action_manifest_url,
            from_file,
            continue_prompt,
            continue_tab,
            no_continue,
            after_update: action_after_update,
        }) => UpdateAction::Install {
            auto: *auto,
            manifest_url: action_manifest_url.clone().or(manifest_url),
            from_file: from_file.clone(),
            continue_prompt: continue_prompt.clone(),
            continue_tab: continue_tab.clone(),
            no_continue: *no_continue,
            after_update: action_after_update.clone().or(after_update),
        },
        None => UpdateAction::Check { force: false },
    }
}

pub(crate) fn run_update_apply_via_mesh(
    target: &str,
    manifest_url: Option<&str>,
    after_update: Option<&str>,
    json_mode: bool,
) {
    let info = match mesh_relay_invoke_result(target, "GET", "/api/update/check?force=true", None) {
        Ok(value) => value,
        Err(error) => error.print_and_exit(json_mode),
    };
    let current = info
        .get("current_version")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let latest = info
        .get("latest_version")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let available = info
        .get("update_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "step": "check",
                "peer": target,
                "data": info,
            }))
            .unwrap_or_default()
        );
    } else {
        println!(
            "[{}] checking…  current v{}, latest v{} — {}",
            target,
            current,
            latest,
            if available {
                "update available"
            } else {
                "up to date"
            }
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
    if let Some(t) = after_update {
        install_path.push_str(&format!("&after_update={}", urlencoding::encode(t)));
    }
    let inner = match mesh_relay_invoke_result(target, "POST", &install_path, None) {
        Ok(value) => value,
        Err(error) => error.print_and_exit(json_mode),
    };
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "step": "install",
                "peer": target,
                "data": inner,
            }))
            .unwrap_or_default()
        );
    } else {
        let s = inner.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let msg = inner.get("message").and_then(|v| v.as_str()).unwrap_or("");
        println!("[{}] applying…  status={} {}", target, s, msg);
    }
}
