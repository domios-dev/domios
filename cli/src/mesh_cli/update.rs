//! Two-step `update install --auto` over mesh: force-check, trigger install,
//! and report the peer's real outcome instead of assuming HTTP 200 succeeded.

use codeterm_types::native_update::apply::{InstallFailureCode, InstallHandoffFailure};

use crate::UpdateAction;

use super::relay::{mesh_relay_invoke_result, MeshSpawnFailure};

const MAX_LEGACY_STATUS_STEPS: usize = 4;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MeshInstallOutcome {
    UpToDate,
    Verified {
        version: String,
    },
    ObserveStatus {
        after_phase: String,
    },
    RestartingUnverified {
        target_version: Option<String>,
        message: String,
    },
    Failed(InstallHandoffFailure),
}

fn verified_peer_version(body: &serde_json::Value) -> Option<String> {
    let phase = &body["native_update"]["phase"];
    let version = phase["payload"]["version"].as_str()?;
    (phase["phase"] == "verified" && body["current_version"].as_str() == Some(version))
        .then(|| version.to_owned())
}

pub(crate) fn classify_mesh_install_response(
    body: &serde_json::Value,
    target_version: Option<&str>,
) -> MeshInstallOutcome {
    let status = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let phase = body.get("phase").and_then(|v| v.as_str()).unwrap_or("");
    if body.get("ok").and_then(|v| v.as_bool()) == Some(false)
        || phase == "error"
        || matches!(
            status,
            "error" | "failed" | "blocked" | "elevation_required"
        )
    {
        if let Ok(existing) = serde_json::from_value::<InstallHandoffFailure>(body.clone()) {
            return MeshInstallOutcome::Failed(existing);
        }
        let raw = body
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("last_error").and_then(|v| v.as_str()))
            .or_else(|| body.get("error").and_then(|v| v.as_str()))
            .unwrap_or(status);
        return MeshInstallOutcome::Failed(InstallHandoffFailure::classify(raw));
    }
    match status {
        "up_to_date" | "no_update" => MeshInstallOutcome::UpToDate,
        "already_in_flight" => MeshInstallOutcome::ObserveStatus {
            after_phase: String::new(),
        },
        "updating" | "scheduled" => {
            if body.get("handoff").and_then(|v| v.as_str()) == Some("relaunching") {
                MeshInstallOutcome::RestartingUnverified {
                    target_version: target_version.map(str::to_owned),
                    message: body
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(
                            "Installer handoff started; replacement version is not yet verified.",
                        )
                        .to_owned(),
                }
            } else {
                // Legacy peer returned 200 before executing; observe status to catch immediate failures.
                MeshInstallOutcome::ObserveStatus {
                    after_phase: "checking".into(),
                }
            }
        }
        other => MeshInstallOutcome::Failed(InstallHandoffFailure {
            code: InstallFailureCode::InstallHandoffFailed,
            message: format!("Peer returned unexpected install status: '{other}'."),
            recovery:
                "Inspect `domios update status --mesh <peer>` and relaunch the peer app if needed."
                    .into(),
            detail: None,
        }),
    }
}

pub(crate) fn classify_mesh_status_response(
    body: &serde_json::Value,
    target_version: Option<&str>,
) -> MeshInstallOutcome {
    let phase = body.get("phase").and_then(|v| v.as_str()).unwrap_or("idle");
    match phase {
        "error" => {
            let raw = body
                .get("last_error")
                .and_then(|v| v.as_str())
                .unwrap_or("update failed");
            MeshInstallOutcome::Failed(InstallHandoffFailure::classify(raw))
        }
        "success" => match verified_peer_version(body) {
            Some(version) => MeshInstallOutcome::Verified { version },
            None => MeshInstallOutcome::RestartingUnverified {
                target_version: target_version.map(str::to_owned),
                message: "Peer reported success without a verified replacement-version receipt."
                    .into(),
            },
        },
        "idle" => {
            if body["native_update"]["phase"]["phase"] == "no_update" {
                MeshInstallOutcome::UpToDate
            } else {
                MeshInstallOutcome::Failed(InstallHandoffFailure {
                    code: InstallFailureCode::InstallHandoffFailed,
                    message:
                        "Peer updater became idle without installing or verifying the new version."
                            .into(),
                    recovery: "Check `domios update status --mesh <peer>` and relaunch the peer app if needed."
                        .into(),
                    detail: None,
                })
            }
        }
        "checking" | "downloading" => MeshInstallOutcome::ObserveStatus {
            after_phase: phase.to_owned(),
        },
        "applying" => MeshInstallOutcome::RestartingUnverified {
            target_version: target_version.map(str::to_owned),
            message: "Installer handoff started; replacement version is not yet verified.".into(),
        },
        other => MeshInstallOutcome::Failed(InstallHandoffFailure {
            code: InstallFailureCode::InstallHandoffFailed,
            message: format!("Peer reported unexpected update phase: '{other}'."),
            recovery: "Check `domios update status --mesh <peer>`.".into(),
            detail: None,
        }),
    }
}

fn observe_peer_outcome(
    target: &str,
    initial_after_phase: String,
    target_version: Option<&str>,
) -> MeshInstallOutcome {
    let mut after_phase = initial_after_phase;
    for step in 0..MAX_LEGACY_STATUS_STEPS {
        let path = if step == 0 && after_phase.is_empty() {
            "/api/update/status".to_owned()
        } else {
            format!(
                "/api/update/wait?after_phase={}",
                urlencoding::encode(&after_phase)
            )
        };
        let status_body = match mesh_relay_invoke_result(target, "GET", &path, None) {
            Ok(body) => body,
            Err(_) => {
                return MeshInstallOutcome::RestartingUnverified {
                    target_version: target_version.map(str::to_owned),
                    message:
                        "Peer connection ended before the replacement version could be verified."
                            .into(),
                };
            }
        };
        match classify_mesh_status_response(&status_body, target_version) {
            MeshInstallOutcome::ObserveStatus {
                after_phase: next_phase,
            } => {
                after_phase = next_phase;
            }
            outcome => return outcome,
        }
    }
    MeshInstallOutcome::RestartingUnverified {
        target_version: target_version.map(str::to_owned),
        message: "Peer update is still in progress; replacement version is not yet verified."
            .into(),
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
            crate::cmd::output::encode(&serde_json::json!({
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
    let target_version = (latest != "?").then_some(latest);
    let outcome = match classify_mesh_install_response(&inner, target_version) {
        MeshInstallOutcome::ObserveStatus { after_phase } => {
            observe_peer_outcome(target, after_phase, target_version)
        }
        other => other,
    };
    match outcome {
        MeshInstallOutcome::UpToDate => {
            if json_mode {
                println!(
                    "{}",
                    crate::cmd::output::encode(&serde_json::json!({
                        "ok": true,
                        "step": "install",
                        "peer": target,
                        "status": "up_to_date",
                        "data": inner,
                    }))
                    .unwrap_or_default()
                );
            } else {
                println!("[{}] already up to date", target);
            }
        }
        MeshInstallOutcome::Verified { version } => {
            if json_mode {
                println!(
                    "{}",
                    crate::cmd::output::encode(&serde_json::json!({
                        "ok": true,
                        "step": "install",
                        "peer": target,
                        "status": "verified",
                        "verified": true,
                        "version": version,
                    }))
                    .unwrap_or_default()
                );
            } else {
                println!("[{}] update verified: v{} is running", target, version);
            }
        }
        MeshInstallOutcome::Failed(failure) => {
            MeshSpawnFailure::refused_before_send(
                target,
                failure.code.as_str(),
                format!("{}: {}", failure.code.as_str(), failure.message),
                failure.recovery,
            )
            .print_and_exit(json_mode);
        }
        MeshInstallOutcome::RestartingUnverified {
            target_version,
            message,
        } => {
            let hint = format!(
                "Run `domios update status --mesh {}` after restart to verify the new version.",
                target
            );
            if json_mode {
                println!(
                    "{}",
                    crate::cmd::output::encode(&serde_json::json!({
                        "ok": false,
                        "step": "install",
                        "peer": target,
                        "status": "relaunching",
                        "verified": false,
                        "target_version": target_version,
                        "message": message,
                        "hint": hint,
                        "data": inner,
                    }))
                    .unwrap_or_default()
                );
            } else {
                eprintln!("[{}] {}\nhint: {}", target, message, hint);
            }
            std::process::exit(2);
        }
        MeshInstallOutcome::ObserveStatus { .. } => unreachable!(),
    }
}

#[cfg(test)]
mod tests;
