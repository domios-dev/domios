//! High-level self-update flow.
//!
//! 1. Resolve the latest release (skip if already on it).
//! 2. Download the platform-specific asset to `data_dir/upgrade-staging/`.
//! 3. Render an orchestrator script that will swap the binary, verify
//!    via /api/health, and roll back from `.bak` on mismatch.
//! 4. Schedule the script with the OS dispatcher (schtasks etc.).
//! 5. Send a "starting" Telegram message via the bridge.
//! 6. Graceful shutdown — script runs after we exit, new daemon picks
//!    up the result file on its next boot.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use super::checker::{self, UpdateInfo};
use super::orchestrator::{self, OrchestratorInputs};
use super::result::{UpgradeContext, UPGRADE_STAGING_DIR};
use super::shutdown;

/// Outcome surfaced back to the caller (REST handler / CLI). We can't
/// observe the actual upgrade — the daemon dies before the script runs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "status")]
pub enum SelfUpdateOutcome {
    #[serde(rename = "up_to_date")]
    UpToDate { version: String },
    #[serde(rename = "scheduled")]
    Scheduled {
        old_version: String,
        new_version: String,
        channel: String,
        log_path: String,
        script_path: String,
    },
    #[serde(rename = "unsupported")]
    Unsupported { reason: String },
}

pub struct SelfUpdateOptions {
    pub channel: super::Channel,
    /// Override download URL (ignored for now — we always go via
    /// `checker::check_for_update`). Kept for forward-compat.
    pub download_url_override: Option<String>,
}

/// Apply step. Mutates state, performs network IO, schedules the
/// external orchestrator, then triggers graceful shutdown.
pub async fn apply(
    app: &tauri::AppHandle,
    opts: SelfUpdateOptions,
) -> Result<SelfUpdateOutcome, String> {
    use crate::api::routes::AppState;
    use tauri::Manager;

    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "AppState not registered".to_string())?
        .inner()
        .clone();

    // Reject early on platforms whose orchestrator is still a stub.
    if !platform_supported() {
        return Ok(SelfUpdateOutcome::Unsupported {
            reason: orchestrator::for_current_os()
                .dispatch(&dummy_inputs())
                .err()
                .unwrap_or_else(|| "platform not yet supported".into()),
        });
    }

    let install_method = state.update_state.lock().install_method.clone();
    let info: UpdateInfo = checker::check_for_update(&install_method)
        .await
        .map_err(|e| format!("update check: {}", e))?;

    if !info.update_available {
        return Ok(SelfUpdateOutcome::UpToDate {
            version: info.current_version,
        });
    }

    let download_url = info
        .download_url
        .clone()
        .ok_or_else(|| "release has no platform asset for this install method".to_string())?;

    let staging_dir = crate::paths::data_dir().join(UPGRADE_STAGING_DIR);
    std::fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("create {}: {}", staging_dir.display(), e))?;
    clear_dir(&staging_dir);

    let installer_path = staging_dir.join(asset_filename(&download_url));
    download_to(&download_url, &installer_path).await?;

    let log_path = crate::paths::data_dir().join(format!(
        "upgrade-{}-to-{}.log",
        info.current_version, info.latest_version
    ));
    let result_path = crate::paths::data_dir().join(super::result::UPGRADE_RESULT_FILENAME);
    // Pre-clean: leftover from a prior run would confuse the boot pickup.
    let _ = std::fs::remove_file(&result_path);

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("current_exe: {}", e))?;

    let (api_port, api_token) = {
        let cfg = state.config.lock();
        (
            cfg.api.port,
            cfg.api.token.clone().unwrap_or_default(),
        )
    };
    let health_url = format!("http://127.0.0.1:{}/api/health", api_port);

    let notify_chat_id = pick_notify_chat_id(&state);

    let context = UpgradeContext {
        old_version: info.current_version.clone(),
        new_version: info.latest_version.clone(),
        channel: opts.channel.to_string(),
        notify_chat_id,
        log_path: log_path.to_string_lossy().to_string(),
        started_at: None,
    };

    let inputs = OrchestratorInputs {
        staging_dir: staging_dir.clone(),
        installer_path,
        current_exe,
        health_url,
        api_token,
        result_path,
        log_path: log_path.clone(),
        context: context.clone(),
    };

    let script_path = orchestrator::for_current_os().dispatch(&inputs)?;

    send_starting_notification(&state, &context).await;

    log::info!(
        "[self-update] script scheduled: {} (log: {})",
        script_path.display(),
        log_path.display()
    );

    // Hand off to the existing graceful-shutdown path. Don't `app.exit()`
    // here — the actual exit happens in the spawned task once shutdown
    // finishes, so the REST response can still serialize first.
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("self-update tokio runtime");
        rt.block_on(async move {
            // Small delay so the REST/Tauri caller sees the response.
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Err(e) = shutdown::graceful_shutdown(&app_handle).await {
                log::error!("[self-update] graceful shutdown failed: {}", e);
            }
            log::info!("[self-update] daemon exiting; orchestrator will take over");
            app_handle.exit(0);
        });
    });

    Ok(SelfUpdateOutcome::Scheduled {
        old_version: context.old_version,
        new_version: context.new_version,
        channel: context.channel,
        log_path: log_path.to_string_lossy().to_string(),
        script_path: script_path.to_string_lossy().to_string(),
    })
}

/// Rollback step. Same orchestrator pattern in reverse: render a script
/// that copies `.bak → install`, restarts, and writes the result.
pub async fn rollback(app: &tauri::AppHandle) -> Result<SelfUpdateOutcome, String> {
    use crate::api::routes::AppState;
    use tauri::Manager;

    if !platform_supported() {
        return Ok(SelfUpdateOutcome::Unsupported {
            reason: "rollback orchestrator not yet implemented for this platform".into(),
        });
    }

    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "AppState not registered".to_string())?
        .inner()
        .clone();

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("current_exe: {}", e))?;

    #[cfg(target_os = "windows")]
    let backup_exe = super::orchestrator::windows::backup_path(&current_exe);
    #[cfg(not(target_os = "windows"))]
    let backup_exe: PathBuf = {
        let mut s = current_exe.as_os_str().to_os_string();
        s.push(".bak");
        PathBuf::from(s)
    };

    if !backup_exe.exists() {
        return Err(format!(
            "no backup at {}; nothing to roll back to",
            backup_exe.display()
        ));
    }

    let staging_dir = crate::paths::data_dir().join(UPGRADE_STAGING_DIR);
    std::fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("create {}: {}", staging_dir.display(), e))?;

    // For rollback we use the .bak as the "installer" by copy. The
    // Windows orchestrator runs `installer /S`; for a rollback we point
    // it at a tiny no-op installer-stub script. Simpler: let the script
    // itself just copy .bak → install and skip the installer step.
    // That's why the rollback flow is its own minimal script — distinct
    // from `--apply`.
    let log_path = crate::paths::data_dir().join("upgrade-rollback.log");
    let result_path = crate::paths::data_dir().join(super::result::UPGRADE_RESULT_FILENAME);
    let _ = std::fs::remove_file(&result_path);

    let (api_port, api_token) = {
        let cfg = state.config.lock();
        (
            cfg.api.port,
            cfg.api.token.clone().unwrap_or_default(),
        )
    };
    let health_url = format!("http://127.0.0.1:{}/api/health", api_port);
    let notify_chat_id = pick_notify_chat_id(&state);

    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let context = UpgradeContext {
        old_version: current_version.clone(),
        new_version: "rollback".into(),
        channel: state.update_state.lock().channel.to_string(),
        notify_chat_id,
        log_path: log_path.to_string_lossy().to_string(),
        started_at: None,
    };

    let script_path = render_and_dispatch_rollback(
        &staging_dir,
        &current_exe,
        &backup_exe,
        &result_path,
        &log_path,
        &health_url,
        &api_token,
        &context,
    )?;

    send_rollback_starting_notification(&state, &context).await;

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("self-update tokio runtime");
        rt.block_on(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Err(e) = shutdown::graceful_shutdown(&app_handle).await {
                log::error!("[self-update] graceful shutdown failed: {}", e);
            }
            log::info!("[self-update] daemon exiting; rollback orchestrator will take over");
            app_handle.exit(0);
        });
    });

    Ok(SelfUpdateOutcome::Scheduled {
        old_version: context.old_version,
        new_version: context.new_version,
        channel: context.channel,
        log_path: log_path.to_string_lossy().to_string(),
        script_path: script_path.to_string_lossy().to_string(),
    })
}

#[cfg(target_os = "windows")]
fn render_and_dispatch_rollback(
    staging_dir: &std::path::Path,
    current_exe: &std::path::Path,
    backup_exe: &std::path::Path,
    result_path: &std::path::Path,
    log_path: &std::path::Path,
    health_url: &str,
    api_token: &str,
    context: &UpgradeContext,
) -> Result<PathBuf, String> {
    let script = render_rollback_ps1(
        current_exe,
        backup_exe,
        result_path,
        log_path,
        health_url,
        api_token,
        context,
    );
    let script_path = staging_dir.join("rollback.ps1");
    std::fs::write(&script_path, script)
        .map_err(|e| format!("write {}: {}", script_path.display(), e))?;

    schedule_powershell_rollback(&script_path)
}

#[cfg(target_os = "windows")]
pub(crate) fn render_rollback_ps1(
    current_exe: &std::path::Path,
    backup_exe: &std::path::Path,
    result_path: &std::path::Path,
    log_path: &std::path::Path,
    health_url: &str,
    api_token: &str,
    context: &UpgradeContext,
) -> String {
    format!(
        r#"# CodeTerm rollback orchestrator
$ErrorActionPreference = 'Continue'
$LogPath      = '{log_path}'
$ResultPath   = '{result_path}'
$CurrentExe   = '{current_exe}'
$BackupExe    = '{backup_exe}'
$HealthUrl    = '{health_url}'
$ApiToken     = '{api_token}'
$OldVersion   = '{old_version}'
$Channel      = '{channel}'
$NotifyChatId = {chat_id_literal}

function Log {{ param([string]$msg)
    $line = ('[{{0}}] {{1}}' -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssZ'), $msg)
    Add-Content -Path $LogPath -Value $line -Encoding UTF8
}}

function Write-Result {{
    param([string]$Status, [string]$Observed, [string]$ErrorMsg, [string]$FinishedAt)
    $obj = [ordered]@{{
        status           = $Status
        old_version      = $OldVersion
        new_version      = 'rollback'
        observed_version = $Observed
        channel          = $Channel
        log_path         = $LogPath
        started_at       = $StartedAt
        finished_at      = $FinishedAt
        error            = $ErrorMsg
        notify_chat_id   = $NotifyChatId
    }}
    Set-Content -Path $ResultPath -Value ($obj | ConvertTo-Json -Depth 4) -Encoding UTF8
}}

$StartedAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
Log "rollback started: current=$OldVersion -> .bak"

# Wait for daemon to die, then nuke survivors.
$deadline = (Get-Date).AddSeconds(30)
while ((Get-Date) -lt $deadline) {{
    if (-not (Get-Process -Name 'codeterm' -ErrorAction SilentlyContinue)) {{ break }}
    Start-Sleep -Milliseconds 500
}}
foreach ($name in @('codeterm', 'codeterm-embed')) {{
    Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object {{
        try {{ $_.Kill() }} catch {{ }}
    }}
}}
Start-Sleep -Seconds 1

if (-not (Test-Path $BackupExe)) {{
    Log 'no .bak — nothing to restore'
    Write-Result 'rollback_failed' '' 'no backup binary found' ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 1
}}

try {{
    Copy-Item -Path $BackupExe -Destination $CurrentExe -Force -ErrorAction Stop
    Log 'restored from .bak'
}} catch {{
    Log "restore failed: $_"
    Write-Result 'rollback_failed' '' "restore failed: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 2
}}

try {{
    Start-Process -FilePath $CurrentExe -WindowStyle Hidden -ErrorAction Stop
    Log 'rollback launched'
}} catch {{
    Log "launch failed: $_"
    Write-Result 'rollback_failed' '' "launch after restore failed: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 3
}}

# Health check.
$deadline = (Get-Date).AddSeconds(60)
$observed = ''
while ((Get-Date) -lt $deadline) {{
    Start-Sleep -Seconds 2
    try {{
        $headers = @{{ Authorization = "Bearer $ApiToken" }}
        $r = Invoke-RestMethod -Uri $HealthUrl -Headers $headers -TimeoutSec 5 -ErrorAction Stop
        if ($r.version) {{ $observed = $r.version ; break }}
    }} catch {{ }}
}}
Write-Result 'rolled_back' $observed '' ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
exit 0
"#,
        log_path = log_path.display(),
        result_path = result_path.display(),
        current_exe = current_exe.display(),
        backup_exe = backup_exe.display(),
        health_url = health_url,
        api_token = api_token.replace('\'', "''"),
        old_version = context.old_version,
        channel = context.channel,
        chat_id_literal = match context.notify_chat_id {
            Some(v) => v.to_string(),
            None => "$null".into(),
        },
    )
}

#[cfg(target_os = "windows")]
fn schedule_powershell_rollback(script_path: &std::path::Path) -> Result<PathBuf, String> {
    use std::process::Command;
    let task_name = "CodeTermSelfUpdateRollback";
    let fire = chrono::Local::now() + chrono::Duration::seconds(120);
    let st = fire.format("%H:%M").to_string();
    let sd = fire.format("%m/%d/%Y").to_string();
    let ps_args = format!(
        "powershell -ExecutionPolicy Bypass -WindowStyle Hidden -NoProfile -File \"{}\"",
        script_path.display()
    );
    let _ = Command::new("schtasks")
        .args(["/Delete", "/TN", task_name, "/F"])
        .output();
    let create = Command::new("schtasks")
        .args([
            "/Create", "/TN", task_name, "/TR", &ps_args,
            "/SC", "ONCE", "/ST", &st, "/SD", &sd,
            "/Z", "/F", "/RL", "LIMITED",
        ])
        .output()
        .map_err(|e| format!("schtasks /Create: {}", e))?;
    if !create.status.success() {
        return Err(format!(
            "schtasks /Create failed: {}",
            String::from_utf8_lossy(&create.stderr)
        ));
    }
    let run = Command::new("schtasks")
        .args(["/Run", "/TN", task_name])
        .output()
        .map_err(|e| format!("schtasks /Run: {}", e))?;
    if !run.status.success() {
        return Err(format!(
            "schtasks /Run failed: {}",
            String::from_utf8_lossy(&run.stderr)
        ));
    }
    Ok(script_path.to_path_buf())
}

#[cfg(not(target_os = "windows"))]
fn render_and_dispatch_rollback(
    _staging_dir: &std::path::Path,
    _current_exe: &std::path::Path,
    _backup_exe: &std::path::Path,
    _result_path: &std::path::Path,
    _log_path: &std::path::Path,
    _health_url: &str,
    _api_token: &str,
    _context: &UpgradeContext,
) -> Result<PathBuf, String> {
    Err("rollback orchestrator not yet implemented for this platform".into())
}

fn platform_supported() -> bool {
    cfg!(target_os = "windows")
}

fn dummy_inputs() -> OrchestratorInputs {
    OrchestratorInputs {
        staging_dir: PathBuf::new(),
        installer_path: PathBuf::new(),
        current_exe: PathBuf::new(),
        health_url: String::new(),
        api_token: String::new(),
        result_path: PathBuf::new(),
        log_path: PathBuf::new(),
        context: UpgradeContext {
            old_version: String::new(),
            new_version: String::new(),
            channel: String::new(),
            notify_chat_id: None,
            log_path: String::new(),
            started_at: None,
        },
    }
}

fn pick_notify_chat_id(state: &crate::api::routes::AppState) -> Option<i64> {
    let cfg = state.config.lock();
    cfg.integrations
        .telegram_allowed_users
        .iter()
        .find_map(|s| s.parse::<i64>().ok())
}

async fn send_starting_notification(
    state: &crate::api::routes::AppState,
    ctx: &UpgradeContext,
) {
    let Some(chat_id) = ctx.notify_chat_id else {
        log::info!("[self-update] no telegram chat configured — skipping starting notification");
        return;
    };
    let text = format!(
        "<b>codeterm self-update starting</b>\nv{} → v{} (channel={})\nlog: {}\nWill report back when done.",
        ctx.old_version, ctx.new_version, ctx.channel, ctx.log_path,
    );
    send_via_bridge(state, chat_id, text).await;
}

async fn send_rollback_starting_notification(
    state: &crate::api::routes::AppState,
    ctx: &UpgradeContext,
) {
    let Some(chat_id) = ctx.notify_chat_id else { return; };
    let text = format!(
        "<b>codeterm rollback starting</b>\nfrom v{} (channel={})\nlog: {}",
        ctx.old_version, ctx.channel, ctx.log_path,
    );
    send_via_bridge(state, chat_id, text).await;
}

async fn send_via_bridge(
    state: &crate::api::routes::AppState,
    chat_id: i64,
    text: String,
) {
    let tx = {
        let guard = state.svc.bridge_outbound.lock();
        guard.clone()
    };
    let Some(tx) = tx else {
        log::info!("[self-update] bridge inactive — skipping notification");
        return;
    };
    let msg = crate::bridge::manager::OutboundMessage {
        user_id: Some(chat_id),
        content: crate::bridge::traits::OutboundContent::Text { text },
        append: false,
        buttons: None,
    };
    if let Err(e) = tx.send(msg) {
        log::warn!("[self-update] bridge send failed: {}", e);
        return;
    }
    // Give the bridge a beat to actually push the message before we kill
    // ourselves. The bridge debounces inbound, but outbound is direct;
    // 750ms is enough for the HTTPS round-trip in practice.
    tokio::time::sleep(Duration::from_millis(750)).await;
}

fn asset_filename(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        "installer.bin".to_string()
    } else {
        last.to_string()
    }
}

fn clear_dir(p: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(p) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

async fn download_to(url: &str, dest: &std::path::Path) -> Result<(), String> {
    log::info!("[self-update] downloading {} -> {}", url, dest.display());
    let client = reqwest::Client::builder()
        .user_agent("CodeTerm-SelfUpdater")
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {}: {}", url, e))?;
    if !resp.status().is_success() {
        return Err(format!("download {}: HTTP {}", url, resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read body: {}", e))?;
    std::fs::write(dest, &bytes).map_err(|e| format!("write {}: {}", dest.display(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_filename_extracts_last_segment() {
        assert_eq!(
            asset_filename("https://example.com/a/b/CodeTerm_1.1.3_x64-setup.exe"),
            "CodeTerm_1.1.3_x64-setup.exe"
        );
        assert_eq!(asset_filename("https://x/y.dmg?token=1"), "y.dmg?token=1");
        assert_eq!(asset_filename(""), "installer.bin");
    }

    #[test]
    fn outcome_serializes_with_status_tag() {
        let s = serde_json::to_string(&SelfUpdateOutcome::UpToDate {
            version: "1.1.2".into(),
        })
        .unwrap();
        assert!(s.contains("\"status\":\"up_to_date\""), "json: {}", s);
        assert!(s.contains("\"version\":\"1.1.2\""));

        let s2 = serde_json::to_string(&SelfUpdateOutcome::Scheduled {
            old_version: "1.1.2".into(),
            new_version: "1.1.3".into(),
            channel: "stable".into(),
            log_path: "/tmp/x.log".into(),
            script_path: "/tmp/run.ps1".into(),
        })
        .unwrap();
        assert!(s2.contains("\"status\":\"scheduled\""));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rollback_script_contains_essentials() {
        use std::path::Path;
        let ctx = UpgradeContext {
            old_version: "1.1.3".into(),
            new_version: "rollback".into(),
            channel: "stable".into(),
            notify_chat_id: Some(42),
            log_path: r"C:\x\rollback.log".into(),
            started_at: None,
        };
        let s = render_rollback_ps1(
            Path::new(r"C:\Program Files\CodeTerm\codeterm.exe"),
            Path::new(r"C:\Program Files\CodeTerm\codeterm.exe.bak"),
            Path::new(r"C:\Users\u\.codeterm\upgrade-result.json"),
            Path::new(r"C:\Users\u\.codeterm\upgrade-rollback.log"),
            "http://127.0.0.1:7685/api/health",
            "TOK",
            &ctx,
        );
        assert!(s.contains("rollback orchestrator"));
        assert!(s.contains("Copy-Item -Path $BackupExe -Destination $CurrentExe"));
        assert!(s.contains("Bearer $ApiToken"));
        assert!(s.contains("$NotifyChatId = 42"));
        assert!(s.contains("'rolled_back'"));
        assert!(s.contains("'rollback_failed'"));
    }
}
