use tauri_plugin_updater::UpdaterExt;
use url::Url;

use super::channel::{
    default_endpoints_for_install_method, immutable_endpoint_for_release, Channel,
};
use super::debian_verify::{
    inspect_package_state, staged_package_name, DebianInstallOutcome, DebianPackageState,
};
use super::manifest;
use super::shutdown;
use super::{InstallMethod, UpdatePhase, UpdateState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    NoUpdate,
    Restarting,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebianElevationStatus {
    Ready,
    ElevationRequired,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DebianPreflight {
    pub status: DebianElevationStatus,
    pub reason: Option<String>,
}

pub enum DebianUpdatePlan {
    NoUpdate,
    Staged {
        version: String,
        path: std::path::PathBuf,
    },
}

pub enum DebianStageGate {
    NoUpdate,
    Ready {
        version: String,
        path: std::path::PathBuf,
    },
    NotReady(DebianPreflight),
}

#[derive(Clone)]
pub struct ApplyOptions {
    pub channel: Channel,
    pub manifest_url: Option<String>,
    pub install_method: InstallMethod,
    /// Tab that initiated the update (the GA/caller running `update install`).
    /// Excluded from the shutdown sweep so it survives to hand off (Defect 3).
    pub initiating_tab_id: Option<String>,
}

pub(crate) fn begin_apply(update_state: &std::sync::Arc<parking_lot::Mutex<UpdateState>>) {
    let mut us = update_state.lock();
    us.set_phase(UpdatePhase::Checking);
    us.clear_error();
}

#[cfg(test)]
mod update_apply_phase_tests {
    use super::*;
    use std::sync::Arc;

    fn state() -> Arc<parking_lot::Mutex<UpdateState>> {
        Arc::new(parking_lot::Mutex::new(UpdateState {
            install_method: InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: Channel::Stable,
            phase: UpdatePhase::Idle,
            last_error: Some("stale error".to_string()),
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        }))
    }

    #[test]
    fn update_apply_phase_begins_checking_and_clears_error() {
        let update_state = state();

        begin_apply(&update_state);

        let state = update_state.lock();
        assert_eq!(state.phase, UpdatePhase::Checking);
        assert!(state.last_error.is_none());
    }
}

pub(super) async fn check_and_apply(
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<ApplyResult, String> {
    begin_apply(&update_state);
    if opts.install_method == InstallMethod::Deb {
        return check_and_apply_deb(app, opts, update_state).await;
    }

    check_and_apply_plugin(app, opts, update_state).await
}

/// Filesystem copies and, on Windows, a `reg` subprocess: never on the Tauri
/// async runtime, where it would stall every other tab's IPC.
async fn refresh_bundled_cli_off_runtime(app: &tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    tokio::task::spawn_blocking(move || crate::startup::refresh_bundled_cli(&handle))
        .await
        .map_err(|e| format!("bundled CLI refresh task failed: {e}"))
}

async fn resolve_manifest_endpoint(opts: &ApplyOptions) -> Result<String, String> {
    if let Some(url) = &opts.manifest_url {
        return Ok(url.clone());
    }
    if opts.channel == Channel::Stable {
        let tag = super::checker::latest_release_tag().await?;
        return Ok(immutable_endpoint_for_release(
            &tag,
            opts.channel,
            &opts.install_method,
        ));
    }
    Ok(default_endpoints_for_install_method(opts.channel, &opts.install_method)[0].clone())
}

async fn check_and_apply_plugin(
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<ApplyResult, String> {
    let endpoint_str = resolve_manifest_endpoint(&opts).await?;

    let endpoint = Url::parse(&endpoint_str)
        .map_err(|e| format!("invalid manifest URL '{}': {}", endpoint_str, e))?;

    let local_version = env!("CARGO_PKG_VERSION");
    let target_arch = std::env::consts::ARCH;
    let target_os = std::env::consts::OS;
    let platform_key = manifest::current_platform_key();

    log::info!(
        "[updater] check_and_apply channel={} url={} local={} os={} arch={} platform={:?}",
        opts.channel,
        endpoint,
        local_version,
        target_os,
        target_arch,
        platform_key,
    );

    let mut builder = app
        .updater_builder()
        .endpoints(vec![endpoint.clone()])
        .map_err(|e| format!("failed to set updater endpoint: {}", e))?;
    if let Some(target) = platform_key {
        builder = builder.target(target);
    }
    let updater = builder
        .build()
        .map_err(|e| format!("failed to build updater: {}", e))?;

    let update = match updater.check().await {
        Ok(u) => u,
        Err(e) => {
            let msg = format!("updater.check() error: {}", e);
            log::error!("[updater] {}", msg);
            return Err(msg);
        }
    };

    let update = match update {
        Some(u) => {
            log::info!(
                "[updater] plugin reported update available: remote_version={} current_version={}",
                u.version,
                u.current_version,
            );
            u
        }
        None => {
            log::warn!(
                "[updater] plugin returned None — running manual manifest probe (local={} os={} arch={})",
                local_version,
                target_os,
                target_arch,
            );
            diagnose_no_update(&endpoint_str, local_version).await;
            return Ok(ApplyResult::NoUpdate);
        }
    };

    update_state.lock().set_phase(UpdatePhase::Downloading);
    log::info!("[updater] downloading and verifying v{}", update.version);
    let chunk_state = update_state.clone();
    let finish_state = update_state.clone();
    let bytes = match update
        .download(
            move |chunk_len, content_length| {
                chunk_state
                    .lock()
                    .record_download_chunk(chunk_len as u64, content_length);
            },
            move || {
                finish_state.lock().set_phase(UpdatePhase::Applying);
            },
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(e) => {
            let msg = format!("download/verify failed: {}", e);
            log::error!("[updater] {}", msg);
            return Err(msg);
        }
    };

    update_state.lock().set_phase(UpdatePhase::Applying);

    // the new app without waiting on the next cold startup (v1.5.6 stale-shim bug).
    refresh_bundled_cli_off_runtime(app).await?;

    let verified_len = bytes.len();
    let initiating_tab_id = opts.initiating_tab_id.clone();
    let sink = super::failure_report::AppFailureSink {
        app: app.clone(),
        update_state: update_state.clone(),
    };

    super::orchestrate::install_after_shutdown(
        bytes,
        // Defect 1: prove the update can land BEFORE any pane is destroyed.
        move || async move {
            tokio::task::spawn_blocking(move || {
                super::viability::plugin_install_viability(verified_len)
            })
            .await
            .map_err(|e| format!("viability probe task failed: {e}"))??;
            super::install_holders::check_before_shutdown(app).await
        },
        || async {
            shutdown::graceful_shutdown(app, initiating_tab_id.as_deref())
                .await
                .map_err(|e| {
                    format!(
                        "installer launch cancelled because PTY shutdown failed: {}",
                        e
                    )
                })
        },
        move |verified_bytes| async move {
            log::info!("[updater] starting verified installer after PTY shutdown");
            tokio::task::spawn_blocking(move || {
                super::analytics::stage_app(&update.version);
                update
                    .install(verified_bytes)
                    .map_err(|e| format!("installer launch failed: {}", e))
            })
            .await
            .map_err(|e| format!("installer launch task failed: {e}"))?
        },
        &sink,
    )
    .await?;

    restart_after_plugin_install(app)
}

#[cfg(windows)]
fn restart_after_plugin_install(_: &tauri::AppHandle) -> Result<ApplyResult, String> {
    // tauri-plugin-updater exits this process after launching the NSIS installer.
    Ok(ApplyResult::Restarting)
}

#[cfg(not(windows))]
fn restart_after_plugin_install(app: &tauri::AppHandle) -> Result<ApplyResult, String> {
    crate::startup::restart_stdio::detach_for_restart();
    app.restart()
}

async fn check_and_apply_deb(
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<ApplyResult, String> {
    let plan = check_stage_deb_update(app, &opts, update_state.clone()).await?;
    let (version, path) = match gate_staged_deb_async(plan, debian_preflight_status).await? {
        DebianStageGate::NoUpdate => return Ok(ApplyResult::NoUpdate),
        DebianStageGate::Ready { path, version } => (version, path),
        DebianStageGate::NotReady(preflight) => {
            return Err(format!(
                "{}: {}",
                debian_status_code(&preflight.status),
                preflight
                    .reason
                    .unwrap_or_else(|| "elevation unavailable".to_string())
            ));
        }
    };

    update_state.lock().set_phase(UpdatePhase::Applying);
    install_staged_deb_and_restart(app, version, path, opts.initiating_tab_id.as_deref()).await?;
    Ok(ApplyResult::Restarting)
}

pub async fn check_stage_deb_update(
    app: &tauri::AppHandle,
    opts: &ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<DebianUpdatePlan, String> {
    begin_apply(&update_state);
    let endpoint_str = resolve_manifest_endpoint(&opts).await?;
    let endpoint = Url::parse(&endpoint_str)
        .map_err(|e| format!("invalid manifest URL '{}': {}", endpoint_str, e))?;

    // Pin the artifact target explicitly so platform selection follows the updater's platform key rather than plugin defaults.
    let mut builder = app
        .updater_builder()
        .endpoints(vec![endpoint.clone()])
        .map_err(|e| format!("failed to set updater endpoint: {}", e))?;
    if let Some(target) = manifest::current_platform_key() {
        builder = builder.target(target);
    }
    let updater = builder
        .build()
        .map_err(|e| format!("failed to build updater: {}", e))?;

    let Some(update) = updater
        .check()
        .await
        .map_err(|e| format!("updater.check() error: {}", e))?
    else {
        diagnose_no_update(&endpoint_str, env!("CARGO_PKG_VERSION")).await;
        return Ok(DebianUpdatePlan::NoUpdate);
    };

    if !is_debian_package_url(&update.download_url) {
        return Err(format!(
            "Debian updater refused non-.deb payload: {}",
            update.download_url
        ));
    }

    update_state.lock().set_phase(UpdatePhase::Downloading);
    log::info!(
        "[updater] downloading and verifying Debian package v{}",
        update.version
    );
    let chunk_state = update_state.clone();
    let bytes = update
        .download(
            move |chunk_len, content_length| {
                chunk_state
                    .lock()
                    .record_download_chunk(chunk_len as u64, content_length);
            },
            || {},
        )
        .await
        .map_err(|e| format!("download/verify failed: {}", e))?;

    let staged = stage_deb_atomically_async(bytes, update.version.clone()).await?;
    Ok(DebianUpdatePlan::Staged {
        version: update.version,
        path: staged,
    })
}

pub async fn gate_staged_deb_async<F>(
    plan: DebianUpdatePlan,
    preflight: F,
) -> Result<DebianStageGate, String>
where
    F: FnOnce() -> DebianPreflight + Send + 'static,
{
    match plan {
        DebianUpdatePlan::NoUpdate => Ok(DebianStageGate::NoUpdate),
        DebianUpdatePlan::Staged { version, path } => {
            let status = tokio::task::spawn_blocking(preflight)
                .await
                .map_err(|e| format!("elevation preflight worker failed: {}", e))?;
            if status.status == DebianElevationStatus::Ready {
                Ok(DebianStageGate::Ready { version, path })
            } else {
                Ok(DebianStageGate::NotReady(status))
            }
        }
    }
}

pub(super) async fn install_staged_deb_and_restart(
    app: &tauri::AppHandle,
    version: String,
    path: std::path::PathBuf,
    initiating_tab_id: Option<&str>,
) -> Result<(), String> {
    let receipt_version = version.clone();
    tokio::task::spawn_blocking(move || super::analytics::stage_app(&receipt_version))
        .await
        .map_err(|e| e.to_string())?;
    install_verified_deb_async(path, version).await?;

    refresh_bundled_cli_off_runtime(app).await?;
    if let Err(e) = shutdown::graceful_shutdown(app, initiating_tab_id).await {
        log::error!("[updater] graceful shutdown failed: {}", e);
    }

    log::info!("[updater] Debian package installed; restarting application");
    crate::startup::restart_stdio::detach_for_restart();
    app.restart()
}

pub fn debian_preflight_status() -> DebianPreflight {
    debian_preflight_status_with(&RealCommandRunner)
}

fn debian_preflight_status_with(runner: &dyn CommandRunner) -> DebianPreflight {
    debian_preflight_status_with_session(runner, has_graphical_session())
}

fn debian_preflight_status_with_session(
    runner: &dyn CommandRunner,
    graphical_session: bool,
) -> DebianPreflight {
    if runner.exists("pkexec") && graphical_session {
        return DebianPreflight {
            status: DebianElevationStatus::Ready,
            reason: Some("pkexec available for session elevation".to_string()),
        };
    }
    if runner.exists("sudo") && runner.run("sudo", &["-n", "true"]).success {
        return DebianPreflight {
            status: DebianElevationStatus::Ready,
            reason: Some("passwordless sudo available".to_string()),
        };
    }
    if graphical_session {
        return DebianPreflight {
            status: DebianElevationStatus::Blocked,
            reason: Some("pkexec is not available for GUI elevation".to_string()),
        };
    }
    DebianPreflight {
        status: DebianElevationStatus::ElevationRequired,
        reason: Some(
            "headless Debian update requires pkexec session or passwordless sudo".to_string(),
        ),
    }
}

fn debian_status_code(status: &DebianElevationStatus) -> &'static str {
    match status {
        DebianElevationStatus::Ready => "ready",
        DebianElevationStatus::ElevationRequired => "elevation_required",
        DebianElevationStatus::Blocked => "blocked",
    }
}

async fn stage_deb_atomically_async(
    bytes: Vec<u8>,
    version: String,
) -> Result<std::path::PathBuf, String> {
    tokio::task::spawn_blocking(move || stage_deb_atomically(&bytes, &version))
        .await
        .map_err(|e| format!("staging worker failed: {}", e))?
}

fn stage_deb_atomically(bytes: &[u8], version: &str) -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("codeterm-updater");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create staging dir {}: {}", dir.display(), e))?;
    let final_path = dir.join(format!("CodeTerm_{}_amd64.deb", version));
    let tmp_path = dir.join(format!(".CodeTerm_{}_{}.tmp", version, std::process::id()));
    std::fs::write(&tmp_path, bytes).map_err(|e| {
        format!(
            "failed to write staged package {}: {}",
            tmp_path.display(),
            e
        )
    })?;
    std::fs::rename(&tmp_path, &final_path).map_err(|e| {
        format!(
            "failed to atomically stage package {} -> {}: {}",
            tmp_path.display(),
            final_path.display(),
            e
        )
    })?;
    Ok(final_path)
}

async fn install_verified_deb_async(
    path: std::path::PathBuf,
    version: String,
) -> Result<(), String> {
    install_verified_deb_with_async(path, version, RealCommandRunner, has_graphical_session()).await
}

async fn install_verified_deb_with_async<R>(
    path: std::path::PathBuf,
    version: String,
    runner: R,
    graphical_session: bool,
) -> Result<(), String>
where
    R: CommandRunner + Send + Sync + 'static,
{
    tokio::task::spawn_blocking(move || {
        install_verified_deb_with(&path, &version, &runner, graphical_session)
    })
    .await
    .map_err(|e| format!("installer worker failed: {}", e))?
}

fn install_verified_deb_with(
    path: &std::path::Path,
    target_version: &str,
    runner: &dyn CommandRunner,
    graphical_session: bool,
) -> Result<(), String> {
    let deb = path
        .to_str()
        .ok_or_else(|| format!("staged package path is not UTF-8: {}", path.display()))?;

    let Some(elevation) = resolve_debian_elevation(runner, graphical_session) else {
        if graphical_session {
            return Err("blocked: pkexec is required for GUI Debian elevation".to_string());
        }
        return Err("elevation_required: headless Debian update requires passwordless sudo or a GUI pkexec session".to_string());
    };

    let package_name = staged_package_name(runner, deb)?;
    let result = run_elevated(runner, &elevation, &["dpkg", "-i", deb]);
    if result.code == Some(126) {
        return Err(format!(
            "elevation_cancelled: authorization cancelled ({})",
            result.stderr
        ));
    }
    if result.code == Some(127) {
        return Err(format!(
            "blocked: elevation command or dpkg not found ({})",
            result.stderr
        ));
    }

    let mut state = inspect_package_state(runner, &package_name);
    if result.success
        && state.install_outcome(result.success, target_version) == DebianInstallOutcome::Verified
    {
        return Ok(());
    }

    if !result.success {
        let _ = run_elevated(runner, &elevation, &["dpkg", "--configure", "-a"]);
        let _ = run_elevated(runner, &elevation, &["apt-get", "install", "-f", "-y"]);
        state = inspect_package_state(runner, &package_name);
        if state.install_outcome(result.success, target_version) == DebianInstallOutcome::Verified {
            return Ok(());
        }
    }

    Err(debian_install_failure_message(
        path,
        target_version,
        &result,
        &state,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DebianElevation {
    Pkexec,
    PasswordlessSudo,
}

fn resolve_debian_elevation(
    runner: &dyn CommandRunner,
    graphical_session: bool,
) -> Option<DebianElevation> {
    if runner.exists("pkexec") && graphical_session {
        Some(DebianElevation::Pkexec)
    } else if runner.exists("sudo") && runner.run("sudo", &["-n", "true"]).success {
        Some(DebianElevation::PasswordlessSudo)
    } else {
        None
    }
}

fn run_elevated(
    runner: &dyn CommandRunner,
    elevation: &DebianElevation,
    command: &[&str],
) -> CommandResult {
    match elevation {
        DebianElevation::Pkexec => {
            let mut args = Vec::with_capacity(command.len());
            args.extend_from_slice(command);
            runner.run("pkexec", &args)
        }
        DebianElevation::PasswordlessSudo => {
            let mut args = Vec::with_capacity(command.len() + 1);
            args.push("-n");
            args.extend_from_slice(command);
            runner.run("sudo", &args)
        }
    }
}

fn debian_install_failure_message(
    path: &std::path::Path,
    target_version: &str,
    install: &CommandResult,
    state: &DebianPackageState,
) -> String {
    format!(
        "{} target_version={} installed_version={} status={} query_exit={:?} query_stderr={:?} audit_success={} audit_exit={:?} audit_stdout={:?} audit_stderr={:?} install_exit={:?} install_stderr={:?} staged_path={}. {}",
        match state.install_outcome(install.success, target_version) {
            DebianInstallOutcome::VerificationFailed => "verification_failed: Debian package installed, but verification failed; restart skipped.",
            DebianInstallOutcome::InstallFailed => "repair_required: Debian package install failed and repair did not verify the target package; restart skipped.",
            DebianInstallOutcome::Verified => unreachable!("verified package does not need an error"),
        },
        target_version,
        state.installed_version.as_deref().unwrap_or("<none>"),
        state.status.as_deref().unwrap_or("<unknown>"),
        state.query.code,
        state.query.stderr,
        state.audit.success,
        state.audit.code,
        state.audit.stdout,
        state.audit.stderr,
        install.code,
        install.stderr,
        path.display(),
        if install.success {
            "Inspect dpkg-query and dpkg --audit diagnostics before attempting manual repair.".to_string()
        } else {
            format!("Manual repair: sudo dpkg --configure -a; sudo apt-get install -f; sudo dpkg -i {}", path.display())
        }
    )
}

fn has_graphical_session() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandResult {
    pub(super) success: bool,
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) trait CommandRunner {
    fn exists(&self, program: &str) -> bool;
    fn run(&self, program: &str, args: &[&str]) -> CommandResult;
    fn run_with_env(&self, program: &str, args: &[&str], env: &[(&str, &str)]) -> CommandResult;
}

struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn exists(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }

    fn run(&self, program: &str, args: &[&str]) -> CommandResult {
        run_command(program, args, &[])
    }

    fn run_with_env(&self, program: &str, args: &[&str], env: &[(&str, &str)]) -> CommandResult {
        run_command(program, args, env)
    }
}

fn run_command(program: &str, args: &[&str], env: &[(&str, &str)]) -> CommandResult {
    let mut command = std::process::Command::new(program);
    command.args(args).envs(env.iter().copied());
    crate::process_util::no_window_std(&mut command);
    match command.output() {
        Ok(out) => CommandResult {
            success: out.status.success(),
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        },
        Err(e) => CommandResult {
            success: false,
            code: None,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

async fn diagnose_no_update(endpoint: &str, local_version: &str) {
    match manifest::fetch(endpoint).await {
        Ok(m) => {
            let plat_key = manifest::current_platform_key();
            let has_payload = plat_key.is_some_and(|key| m.platforms.contains_key(key));
            log::warn!(
                "[updater] manual manifest probe: remote_version={} platform_key={} has_payload_for_platform={} platforms={:?}",
                m.version,
                plat_key.unwrap_or("<unknown>"),
                has_payload,
                m.platforms.keys().collect::<Vec<_>>(),
            );
            // The diagnostic must follow the same platform artifact rule as apply, not a bare version comparison.
            match manifest::offered_version(local_version, &m, plat_key) {
                Ok(Some(v)) => log::error!(
                    "[updater] DIAGNOSTIC: manual compare says local {} < remote {} with a payload for '{}' (UPDATE AVAILABLE) but plugin returned None — likely a signature issue",
                    local_version, v, plat_key.unwrap_or("<unknown>"),
                ),
                Ok(None) if has_payload => log::info!(
                    "[updater] manual compare confirms local {} >= remote {} — already up to date",
                    local_version, m.version,
                ),
                Ok(None) => log::info!(
                    "[updater] manual compare: remote {} has no payload for platform '{}' — nothing to offer this client",
                    m.version, plat_key.unwrap_or("<unknown>"),
                ),
                Err(e) => log::error!("[updater] manual diff parse error: {}", e),
            }
        }
        Err(e) => log::warn!("[updater] manual manifest fetch failed: {}", e),
    }
}

/// The Debian updater must never hand `dpkg -i` anything but the `.deb` bytes
/// it was verified to fetch. `Url::path()` is percent-encoded but unaffected
/// here since GitHub release asset names are already URL-safe.
fn is_debian_package_url(url: &Url) -> bool {
    url.path().ends_with(".deb")
}

#[cfg(test)]
mod debian_tests;
