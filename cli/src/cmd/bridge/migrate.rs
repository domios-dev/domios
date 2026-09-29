use std::time::Instant;

use serde::Deserialize;

use crate::{build_local_api, local_api::LocalApiClient, read_api_config};

pub struct MigrateArgs {
    pub to: String,
    pub service: String,
    pub announce: bool,
    pub announce_chat_id: Option<i64>,
    pub no_rollback: bool,
    pub force: bool,
    pub claim_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct BridgeState {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub token_present: bool,
    #[serde(default)]
    pub has_secret_in_vault: bool,
    #[serde(default)]
    pub secret_name: Option<String>,
    #[serde(default)]
    pub error_count: i64,
    #[serde(default)]
    pub last_poll_ok: Option<String>,
    #[serde(default)]
    pub instance_name: Option<String>,
}

const EXIT_TARGET_MISCONFIG: i32 = 5;
const EXIT_USAGE: i32 = 6;
const EXIT_SPLIT_BRAIN: i32 = 7;

pub fn run_migrate(json: bool, args: MigrateArgs) {
    let started = Instant::now();
    if std::env::var("CODETERM_MESH_TARGET")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
    {
        crate::cmd::print::fail(
            EXIT_USAGE,
            "bridge migrate must run locally on the source machine. Aborting.",
        );
    }

    let (base, _) = read_api_config();
    let api = match build_local_api() {
        Ok(c) => c,
        Err(e) => {
            crate::cmd::print::fail(1, format!("failed to connect to local API: {}", e));
        }
    };

    let source_state = match get_bridge_state_local(&api, &base, &args.service) {
        Ok(s) => s,
        Err(e) => {
            crate::cmd::print::fail_with_hints(
                EXIT_TARGET_MISCONFIG,
                format!("bridge subsystem not available locally: {}", e),
                &["hint: update the daemon — /api/bridge/state may not exist on this build."],
            );
        }
    };

    let source_name = match source_state.instance_name.clone() {
        Some(n) if !n.is_empty() => n,
        _ => {
            crate::cmd::print::fail(EXIT_TARGET_MISCONFIG, "/api/bridge/state did not include instance_name. Daemon may be older than the bridge migrate feature.");
        }
    };

    if let Some(msg) = check_idempotent(&source_name, &args.to) {
        log_step(json, "noop", &msg);
        return;
    }

    let target_id = match resolve_paired_peer(&api, &base, &args.to) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(EXIT_TARGET_MISCONFIG);
        }
    };

    if let Err(e) = relay_get(&target_id, "/api/health") {
        crate::cmd::print::fail(
            EXIT_TARGET_MISCONFIG,
            format!("target {} unreachable: {}", args.to, e),
        );
    }
    log_step(json, "preflight", &format!("target {} reachable", args.to));

    let target_state = match relay_get_state(&target_id, &args.service) {
        Ok(s) => s,
        Err(e) => {
            crate::cmd::print::fail(
                EXIT_TARGET_MISCONFIG,
                format!("target {} bridge state unavailable: {}", args.to, e),
            );
        }
    };

    if !args.force {
        if let Err(msg) = target_can_serve(&target_state) {
            crate::cmd::print::fail(
                EXIT_TARGET_MISCONFIG,
                format!("target {}: {}", args.to, msg),
            );
        }
    }

    if check_already_migrated(&source_state, &target_state) {
        log_step(
            json,
            "noop",
            &format!(
                "already migrated; target {} is active and source is not",
                args.to
            ),
        );
        return;
    }

    if let Err(e) = patch_config_local(
        &api,
        &base,
        "integrations.telegram_enabled",
        serde_json::json!(false),
    ) {
        eprintln!(
            "Error: failed to persist source telegram_enabled=false: {}",
            e
        );
        eprintln!("hint: aborting before stopping the poller — source remains active.");
        std::process::exit(1);
    }
    log_step(json, "config", "source telegram_enabled=false");

    let stop_started = Instant::now();
    let stop_resp = post_local(
        &api,
        &base,
        "/api/bridge/stop",
        &serde_json::json!({"service": args.service, "timeout_ms": 30000}),
    );
    let released_ms = match stop_resp {
        Ok(v) => v
            .get("released_in_ms")
            .and_then(|x| x.as_u64())
            .unwrap_or_else(|| stop_started.elapsed().as_millis() as u64),
        Err(e) => {
            crate::cmd::print::fail(1, format!("source release failed: {}", e));
        }
    };
    log_step(json, "stop", &format!("released in {}ms", released_ms));

    if let Err(e) = relay_patch_config(
        &target_id,
        "integrations.telegram_enabled",
        serde_json::json!(true),
    ) {
        eprintln!(
            "Warning: failed to persist target telegram_enabled=true: {}",
            e
        );
        eprintln!("hint: /api/bridge/start will still flip it; future reboots may not auto-start.");
    }
    log_step(json, "config", "target telegram_enabled=true");

    let claim_started = Instant::now();
    let claim_resp = relay_post(
        &target_id,
        "/api/bridge/start",
        &serde_json::json!({
            "service": args.service,
            "ensure_token": true,
            "wait_for_lock_ms": args.claim_timeout_ms,
        }),
    );
    let claim_ms = claim_started.elapsed().as_millis() as u64;
    let claimed_lock = match &claim_resp {
        Ok(v) => v
            .get("claimed_lock")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        Err(_) => false,
    };
    if claim_resp.is_err() || !claimed_lock {
        let err_msg = claim_resp
            .err()
            .unwrap_or_else(|| "claim returned ok=false".to_string());
        eprintln!("Error: target claim failed: {}", err_msg);
        if args.no_rollback {
            eprintln!("--no-rollback set: leaving source disabled.");
            std::process::exit(1);
        }
        match rollback(&api, &base, &args.service, args.claim_timeout_ms) {
            Ok(()) => {
                eprintln!("Rollback: succeeded (source is active again)");
                std::process::exit(1);
            }
            Err(e) => {
                report_split_brain(json, &source_name, &args.to, &args.service, &e);
                std::process::exit(EXIT_SPLIT_BRAIN);
            }
        }
    }
    log_step(
        json,
        "start",
        &format!("target claimed lock in {}ms", claim_ms),
    );

    let verify_local = get_bridge_state_local(&api, &base, &args.service);
    let verify_target = relay_get_state(&target_id, &args.service);
    let local_inactive = verify_local.as_ref().map(|s| !s.active).unwrap_or(false);
    let target_active = verify_target.as_ref().map(|s| s.active).unwrap_or(false);
    if !local_inactive || !target_active {
        eprintln!(
            "Error: post-migration verification failed (local inactive={}, target active={}).",
            local_inactive, target_active
        );
        if args.no_rollback {
            std::process::exit(1);
        }
        match rollback(&api, &base, &args.service, args.claim_timeout_ms) {
            Ok(()) => {
                eprintln!("Rollback: succeeded");
                std::process::exit(1);
            }
            Err(e) => {
                report_split_brain(json, &source_name, &args.to, &args.service, &e);
                std::process::exit(EXIT_SPLIT_BRAIN);
            }
        }
    }

    if args.announce {
        let payload = serde_json::json!({
            "service": args.service,
            "chat_id": args.announce_chat_id,
            "text": format!(
                "[CodeTerm] Telegram bridge migrated to {} from {}",
                args.to, source_name
            ),
        });
        if let Err(e) = relay_post(&target_id, "/api/bridge/announce", &payload) {
            eprintln!("Warning: announce failed (non-fatal): {}", e);
        } else {
            log_step(json, "announce", "queued sentinel message on target");
        }
    }

    let total_ms = started.elapsed().as_millis() as u64;
    if json {
        let summary = serde_json::json!({
            "ok": true,
            "service": args.service,
            "from": source_name,
            "to": args.to,
            "to_instance_id": target_id,
            "released_in_ms": released_ms,
            "claimed_in_ms": claim_ms,
            "total_ms": total_ms,
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    } else {
        println!(
            "Migrated {} from {} -> {} in {}ms (released in {}ms, claimed in {}ms)",
            args.service, source_name, args.to, total_ms, released_ms, claim_ms
        );
    }
}

fn rollback(
    api: &LocalApiClient,
    base: &str,
    service: &str,
    claim_timeout_ms: u64,
) -> Result<(), String> {
    if let Err(e) = patch_config_local(
        api,
        base,
        "integrations.telegram_enabled",
        serde_json::json!(true),
    ) {
        return Err(format!("config re-enable: {}", e));
    }
    post_local(
        api,
        base,
        "/api/bridge/start",
        &serde_json::json!({
            "service": service,
            "ensure_token": true,
            "wait_for_lock_ms": claim_timeout_ms,
        }),
    )
    .map(|_| ())
    .map_err(|e| format!("bridge start: {}", e))
}

fn log_step(json: bool, step: &str, msg: &str) {
    if !json {
        eprintln!("[migrate:{}] {}", step, msg);
    }
}

fn report_split_brain(json: bool, source: &str, target: &str, service: &str, rollback_err: &str) {
    let repair_cmd = format!(
        "codeterm config set integrations.{}_enabled true && codeterm bridge restart",
        service
    );
    if json {
        let payload = serde_json::json!({
            "ok": false,
            "state": "orphaned",
            "service": service,
            "from": source,
            "to": target,
            "rollback_error": rollback_err,
            "repair_command": repair_cmd,
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return;
    }
    eprintln!();
    eprintln!("============================================================");
    eprintln!("MANUAL REPAIR REQUIRED — split-brain state");
    eprintln!("------------------------------------------------------------");
    eprintln!("  source: {} (disabled, poller stopped)", source);
    eprintln!("  target: {} (claim failed, not active)", target);
    eprintln!("  rollback error: {}", rollback_err);
    eprintln!();
    eprintln!("To restore service on the source, run:");
    eprintln!("  {}", repair_cmd);
    eprintln!("============================================================");
}

pub fn check_idempotent(source_name: &str, target_name: &str) -> Option<String> {
    if source_name.eq_ignore_ascii_case(target_name) {
        Some(format!("Already on {}; nothing to do.", source_name))
    } else {
        None
    }
}

pub fn target_can_serve(state: &BridgeState) -> Result<(), String> {
    if state.token_present || state.has_secret_in_vault {
        Ok(())
    } else {
        let secret = state.secret_name.as_deref().unwrap_or("telegram_bot_token");
        Err(format!(
            "no token and no Bitwarden secret '{}' in vault. Aborting.",
            secret
        ))
    }
}

pub fn check_already_migrated(source: &BridgeState, target: &BridgeState) -> bool {
    !source.active && target.active
}

fn get_bridge_state_local(
    api: &LocalApiClient,
    base: &str,
    service: &str,
) -> Result<BridgeState, String> {
    let url = format!("{}/api/bridge/state?service={}", base, service);
    let resp = api.get(&url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    resp.json::<BridgeState>().map_err(|e| e.to_string())
}

fn resolve_paired_peer(api: &LocalApiClient, base: &str, target: &str) -> Result<String, String> {
    let val: serde_json::Value = api
        .get(format!("{}/api/mesh/peers", base))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let paired = val
        .get("paired")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no paired peers in /api/mesh/peers response".to_string())?;
    let peer = paired.iter().find(|p| {
        p.get("instance_name").and_then(|v| v.as_str()) == Some(target)
            || p.get("instance_id").and_then(|v| v.as_str()) == Some(target)
    });
    let Some(peer) = peer else {
        let names: Vec<&str> = paired
            .iter()
            .filter_map(|p| p.get("instance_name").and_then(|v| v.as_str()))
            .collect();
        return Err(format!(
            "Error: paired peer '{}' not found. Known: [{}]",
            target,
            names.join(", ")
        ));
    };
    let id = peer
        .get("instance_id")
        .and_then(|v| v.as_str())
        .unwrap_or(target)
        .to_string();
    Ok(id)
}

fn post_local(
    api: &LocalApiClient,
    base: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = api
        .post(format!("{}{}", base, path))
        .json(body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    if status.is_success() {
        Ok(val)
    } else {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
        Err(format!("HTTP {}: {}", status.as_u16(), preview.trim()))
    }
}

fn patch_config_local(
    api: &LocalApiClient,
    base: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    let body = serde_json::json!({"key": key, "value": value});
    let resp = api
        .patch(format!("{}/api/config", base))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status().as_u16()))
    }
}

fn relay_send(
    target: &str,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let body = body.map(serde_json::Value::to_string);
    crate::mesh_cli::mesh_relay_invoke_result(target, method, path, body.as_deref())
        .map_err(|error| error.to_string())
}

fn relay_get(target: &str, path: &str) -> Result<serde_json::Value, String> {
    relay_send(target, "GET", path, None)
}

fn relay_post(
    target: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    relay_send(target, "POST", path, Some(body))
}

fn relay_patch_config(
    target: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({"key": key, "value": value});
    relay_send(target, "PATCH", "/api/config", Some(&body))
}

fn relay_get_state(target: &str, service: &str) -> Result<BridgeState, String> {
    let val = relay_get(target, &format!("/api/bridge/state?service={}", service))?;
    serde_json::from_value::<BridgeState>(val).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(active: bool, token_present: bool, has_vault: bool) -> BridgeState {
        BridgeState {
            active,
            token_present,
            has_secret_in_vault: has_vault,
            secret_name: Some("telegram_bot_token".into()),
            error_count: 0,
            last_poll_ok: None,
            instance_name: None,
        }
    }

    #[test]
    fn idempotent_when_target_equals_source() {
        let msg = check_idempotent("BLACKSTAR", "BLACKSTAR");
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("BLACKSTAR"));
    }

    #[test]
    fn idempotent_is_case_insensitive() {
        assert!(check_idempotent("BLACKSTAR", "blackstar").is_some());
    }

    #[test]
    fn idempotent_returns_none_when_different() {
        assert!(check_idempotent("BLACKSTAR", "rollacode-bot").is_none());
    }

    #[test]
    fn target_can_serve_with_token() {
        assert!(target_can_serve(&state(false, true, false)).is_ok());
    }

    #[test]
    fn target_can_serve_with_vault_secret() {
        assert!(target_can_serve(&state(false, false, true)).is_ok());
    }

    #[test]
    fn target_cannot_serve_without_token_or_vault() {
        let err = target_can_serve(&state(false, false, false)).unwrap_err();
        assert!(err.contains("telegram_bot_token"));
        assert!(err.contains("Aborting"));
    }

    #[test]
    fn target_cannot_serve_uses_provided_secret_name() {
        let mut s = state(false, false, false);
        s.secret_name = Some("custom_secret".into());
        let err = target_can_serve(&s).unwrap_err();
        assert!(err.contains("custom_secret"));
    }

    #[test]
    fn already_migrated_when_source_off_target_on() {
        assert!(check_already_migrated(
            &state(false, true, true),
            &state(true, true, true)
        ));
    }

    #[test]
    fn not_already_migrated_when_source_on() {
        assert!(!check_already_migrated(
            &state(true, true, true),
            &state(true, true, true)
        ));
    }

    #[test]
    fn not_already_migrated_when_target_off() {
        assert!(!check_already_migrated(
            &state(false, true, true),
            &state(false, true, true)
        ));
    }

    #[test]
    fn bridge_state_deserializes_minimal_payload() {
        let raw = serde_json::json!({"active": true, "token_present": true});
        let parsed: BridgeState = serde_json::from_value(raw).unwrap();
        assert!(parsed.active);
        assert!(parsed.token_present);
        assert!(!parsed.has_secret_in_vault);
        assert_eq!(parsed.error_count, 0);
    }

    #[test]
    fn bridge_state_deserializes_full_payload() {
        let raw = serde_json::json!({
            "active": false,
            "token_present": false,
            "has_secret_in_vault": true,
            "secret_name": "telegram_bot_token",
            "error_count": 3,
            "last_poll_ok": "2026-05-06T12:00:00Z"
        });
        let parsed: BridgeState = serde_json::from_value(raw).unwrap();
        assert!(!parsed.active);
        assert!(parsed.has_secret_in_vault);
        assert_eq!(parsed.error_count, 3);
        assert_eq!(parsed.last_poll_ok.as_deref(), Some("2026-05-06T12:00:00Z"));
    }
}
