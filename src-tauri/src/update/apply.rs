use tauri_plugin_updater::UpdaterExt;
use url::Url;

use super::channel::{
    default_endpoints_for_install_method, immutable_endpoint_for_release, Channel,
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

pub async fn check_and_apply(
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<ApplyResult, String> {
    if opts.install_method == InstallMethod::Deb {
        return check_and_apply_deb(app, opts, update_state).await;
    }

    check_and_apply_plugin(app, opts, update_state).await
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

    log::info!(
        "[updater] check_and_apply channel={} url={} local={} os={} arch={}",
        opts.channel,
        endpoint,
        local_version,
        target_os,
        target_arch,
    );

    let builder = app
        .updater_builder()
        .endpoints(vec![endpoint.clone()])
        .map_err(|e| format!("failed to set updater endpoint: {}", e))?;
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
    crate::startup::refresh_bundled_cli(app);

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
            .map_err(|e| format!("viability probe task failed: {e}"))?
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
        |verified_bytes| {
            log::info!("[updater] starting verified installer after PTY shutdown");
            update
                .install(verified_bytes)
                .map_err(|e| format!("installer launch failed: {}", e))
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
    let endpoint_str = resolve_manifest_endpoint(&opts).await?;
    let endpoint = Url::parse(&endpoint_str)
        .map_err(|e| format!("invalid manifest URL '{}': {}", endpoint_str, e))?;

    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint.clone()])
        .map_err(|e| format!("failed to set updater endpoint: {}", e))?
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

    if !update.download_url.path().ends_with(".deb") {
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

pub async fn install_staged_deb_and_restart(
    app: &tauri::AppHandle,
    version: String,
    path: std::path::PathBuf,
    initiating_tab_id: Option<&str>,
) -> Result<(), String> {
    install_verified_deb_async(path, version).await?;

    crate::startup::refresh_bundled_cli(app);
    if let Err(e) = shutdown::graceful_shutdown(app, initiating_tab_id).await {
        log::error!("[updater] graceful shutdown failed: {}", e);
    }

    log::info!("[updater] Debian package installed; restarting application");
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

    let mut state = inspect_debian_package_state(runner);
    if result.success && state.is_consistent(target_version) {
        return Ok(());
    }

    if !result.success {
        let _ = run_elevated(runner, &elevation, &["dpkg", "--configure", "-a"]);
        let _ = run_elevated(runner, &elevation, &["apt-get", "install", "-f", "-y"]);
        state = inspect_debian_package_state(runner);
        if state.is_consistent(target_version) {
            return Ok(());
        }
    }

    Err(repair_required_message(
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct DebianPackageState {
    installed_version: Option<String>,
    status: Option<String>,
    audit: CommandResult,
}

impl DebianPackageState {
    fn is_consistent(&self, target_version: &str) -> bool {
        self.installed_version.as_deref() == Some(target_version)
            && self.status.as_deref() == Some("install ok installed")
            && self.audit.success
            && self.audit.stdout.trim().is_empty()
    }
}

fn inspect_debian_package_state(runner: &dyn CommandRunner) -> DebianPackageState {
    let version = runner.run("dpkg-query", &["-W", "-f=${Version}", "codeterm"]);
    let status = runner.run(
        "dpkg-query",
        &["-W", "-f=${db:Status-Abbrev} ${Status}", "codeterm"],
    );
    let audit = runner.run("dpkg", &["--audit"]);
    DebianPackageState {
        installed_version: version.success.then(|| version.stdout.trim().to_string()),
        status: status
            .success
            .then(|| normalize_dpkg_status(&status.stdout)),
        audit,
    }
}

fn normalize_dpkg_status(raw: &str) -> String {
    raw.trim()
        .strip_prefix("ii ")
        .unwrap_or(raw.trim())
        .trim()
        .to_string()
}

fn repair_required_message(
    path: &std::path::Path,
    target_version: &str,
    install: &CommandResult,
    state: &DebianPackageState,
) -> String {
    format!(
        "repair_required: Debian package consistency could not be proven after install attempt. target_version={} installed_version={} status={} audit_success={} audit_exit={:?} audit_stdout={:?} audit_stderr={:?} install_exit={:?} install_stderr={:?} staged_path={}. Manual repair: sudo dpkg --configure -a; sudo apt-get install -f; sudo dpkg -i {}",
        target_version,
        state.installed_version.as_deref().unwrap_or("<none>"),
        state.status.as_deref().unwrap_or("<unknown>"),
        state.audit.success,
        state.audit.code,
        state.audit.stdout,
        state.audit.stderr,
        install.code,
        install.stderr,
        path.display(),
        path.display()
    )
}

fn has_graphical_session() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandResult {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

trait CommandRunner {
    fn exists(&self, program: &str) -> bool;
    fn run(&self, program: &str, args: &[&str]) -> CommandResult;
}

struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn exists(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }

    fn run(&self, program: &str, args: &[&str]) -> CommandResult {
        let mut command = std::process::Command::new(program);
        command.args(args);
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
}

async fn diagnose_no_update(endpoint: &str, local_version: &str) {
    match manifest::fetch(endpoint).await {
        Ok(m) => {
            let plat_key = manifest::current_platform_key().unwrap_or("<unknown>");
            let has_payload = m.platforms.contains_key(plat_key);
            log::warn!(
                "[updater] manual manifest probe: remote_version={} platform_key={} has_payload_for_platform={} platforms={:?}",
                m.version,
                plat_key,
                has_payload,
                m.platforms.keys().collect::<Vec<_>>(),
            );
            match manifest::diff(local_version, &m) {
                Ok(Some(v)) => log::error!(
                    "[updater] DIAGNOSTIC: manual compare says local {} < remote {} (UPDATE AVAILABLE) but plugin returned None — likely missing/mismatched platform key '{}' or signature issue",
                    local_version, v, plat_key,
                ),
                Ok(None) => log::info!(
                    "[updater] manual compare confirms local {} >= remote {} — already up to date",
                    local_version, m.version,
                ),
                Err(e) => log::error!("[updater] manual diff parse error: {}", e),
            }
        }
        Err(e) => log::warn!("[updater] manual manifest fetch failed: {}", e),
    }
}

#[cfg(test)]
mod debian_tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    struct FakeRunner {
        exists: HashSet<String>,
        results: HashMap<String, CommandResult>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(programs: &[&str]) -> Self {
            Self {
                exists: programs.iter().map(|s| s.to_string()).collect(),
                results: HashMap::new(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn with_result(mut self, key: &str, result: CommandResult) -> Self {
            self.results.insert(key.to_string(), result);
            self
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn exists(&self, program: &str) -> bool {
            self.exists.contains(program)
        }

        fn run(&self, program: &str, args: &[&str]) -> CommandResult {
            let key = format!("{} {}", program, args.join(" "));
            self.calls.borrow_mut().push(key.clone());
            self.results.get(&key).cloned().unwrap_or(CommandResult {
                success: true,
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    fn ok(stdout: &str) -> CommandResult {
        CommandResult {
            success: true,
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn fail(code: i32, stderr: &str) -> CommandResult {
        CommandResult {
            success: false,
            code: Some(code),
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    fn clean_state(runner: FakeRunner, version: &str) -> FakeRunner {
        runner
            .with_result("dpkg-query -W -f=${Version} codeterm", ok(version))
            .with_result(
                "dpkg-query -W -f=${db:Status-Abbrev} ${Status} codeterm",
                ok("ii install ok installed"),
            )
            .with_result("dpkg --audit", ok(""))
    }

    #[test]
    fn passwordless_sudo_installs_without_password_prompt() {
        let runner = clean_state(FakeRunner::new(&["sudo"]), "1.7.1");
        let path = std::path::Path::new("/tmp/CodeTerm_1.7.1_amd64.deb");
        install_verified_deb_with(path, "1.7.1", &runner, false).unwrap();
        let calls = runner.calls();
        assert!(calls.contains(&"sudo -n true".to_string()));
        assert!(calls.contains(&"sudo -n dpkg -i /tmp/CodeTerm_1.7.1_amd64.deb".to_string()));
    }

    #[test]
    fn password_required_sudo_returns_elevation_required() {
        let runner = FakeRunner::new(&["sudo"])
            .with_result("sudo -n true", fail(1, "a password is required"));
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
                .unwrap_err();
        assert!(err.contains("elevation_required"));
    }

    #[test]
    fn headless_preflight_returns_elevation_required() {
        let runner = FakeRunner::new(&["sudo"])
            .with_result("sudo -n true", fail(1, "a password is required"));
        let preflight = debian_preflight_status_with_session(&runner, false);
        assert_eq!(preflight.status, DebianElevationStatus::ElevationRequired);
    }

    #[test]
    fn gui_preflight_blocks_without_pkexec() {
        let runner = FakeRunner::new(&[]);
        let preflight = debian_preflight_status_with_session(&runner, true);
        assert_eq!(preflight.status, DebianElevationStatus::Blocked);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn no_update_does_not_run_elevation_preflight() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let gate = gate_staged_deb_async(DebianUpdatePlan::NoUpdate, move || {
            flag.store(true, Ordering::SeqCst);
            DebianPreflight {
                status: DebianElevationStatus::ElevationRequired,
                reason: Some("should not be observed".to_string()),
            }
        })
        .await
        .unwrap();
        assert!(matches!(gate, DebianStageGate::NoUpdate));
        assert!(!called.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn staged_runs_preflight_through_async_boundary() {
        let gate = gate_staged_deb_async(
            DebianUpdatePlan::Staged {
                version: "1.7.1".to_string(),
                path: std::path::PathBuf::from("/tmp/CodeTerm_1.7.1_amd64.deb"),
            },
            || DebianPreflight {
                status: DebianElevationStatus::Ready,
                reason: Some("passwordless sudo available".to_string()),
            },
        )
        .await
        .unwrap();
        assert!(matches!(gate, DebianStageGate::Ready { .. }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn staged_not_ready_surfaces_structured_status() {
        let gate = gate_staged_deb_async(
            DebianUpdatePlan::Staged {
                version: "1.7.1".to_string(),
                path: std::path::PathBuf::from("/tmp/a.deb"),
            },
            || DebianPreflight {
                status: DebianElevationStatus::ElevationRequired,
                reason: Some("headless requires elevation".to_string()),
            },
        )
        .await
        .unwrap();
        match gate {
            DebianStageGate::NotReady(p) => {
                assert_eq!(p.status, DebianElevationStatus::ElevationRequired)
            }
            _ => panic!("expected NotReady"),
        }
    }

    #[test]
    fn pkexec_cancelled_is_not_success() {
        let runner = FakeRunner::new(&["pkexec"]).with_result(
            "pkexec dpkg -i /tmp/a.deb",
            fail(126, "dismissed authentication dialog"),
        );
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, true)
                .unwrap_err();
        assert!(err.contains("elevation_cancelled"));
    }

    #[test]
    fn command_not_found_is_blocked_not_cancelled() {
        let runner = FakeRunner::new(&["pkexec"])
            .with_result("pkexec dpkg -i /tmp/a.deb", fail(127, "pkexec: not found"));
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, true)
                .unwrap_err();
        assert!(err.contains("blocked"));
        assert!(!err.contains("elevation_cancelled"));
    }

    #[test]
    fn half_configured_state_repairs_successfully() {
        let runner = clean_state(
            FakeRunner::new(&["sudo"])
                .with_result("sudo -n dpkg -i /tmp/a.deb", fail(2, "postinst failed"))
                .with_result("sudo -n dpkg --configure -a", ok(""))
                .with_result("sudo -n apt-get install -f -y", ok("")),
            "1.7.1",
        );
        install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
            .unwrap();
        let calls = runner.calls();
        assert!(calls.contains(&"sudo -n dpkg --configure -a".to_string()));
        assert!(calls.contains(&"sudo -n apt-get install -f -y".to_string()));
    }

    #[test]
    fn repair_failure_without_prior_package_returns_repair_required() {
        let runner = FakeRunner::new(&["sudo"])
            .with_result("sudo -n dpkg -i /tmp/a.deb", fail(2, "postinst failed"))
            .with_result("sudo -n dpkg --configure -a", fail(1, "still broken"))
            .with_result("sudo -n apt-get install -f -y", fail(1, "no fix"))
            .with_result(
                "dpkg-query -W -f=${Version} codeterm",
                fail(1, "not installed"),
            )
            .with_result(
                "dpkg-query -W -f=${db:Status-Abbrev} ${Status} codeterm",
                fail(1, "not installed"),
            )
            .with_result("dpkg --audit", ok("codeterm is half-configured"));
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
                .unwrap_err();
        assert!(err.contains("repair_required"));
        assert!(err.contains("staged_path=/tmp/a.deb"));
        assert!(err.contains("sudo dpkg --configure -a"));
    }

    #[test]
    fn resulting_version_mismatch_returns_repair_required() {
        let runner = clean_state(FakeRunner::new(&["sudo"]), "1.6.1");
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
                .unwrap_err();
        assert!(err.contains("repair_required"));
        assert!(err.contains("installed_version=1.6.1"));
    }

    #[test]
    fn failed_empty_audit_is_repair_required() {
        let runner = FakeRunner::new(&["sudo"])
            .with_result("dpkg-query -W -f=${Version} codeterm", ok("1.7.1"))
            .with_result(
                "dpkg-query -W -f=${db:Status-Abbrev} ${Status} codeterm",
                ok("ii install ok installed"),
            )
            .with_result("dpkg --audit", fail(2, "audit backend failed"));
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
                .unwrap_err();
        assert!(err.contains("repair_required"));
        assert!(err.contains("audit_success=false"));
        assert!(err.contains("audit_exit=Some(2)"));
        assert!(err.contains("audit backend failed"));
    }

    #[test]
    fn dpkg_query_failure_is_repair_required() {
        let runner = FakeRunner::new(&["sudo"])
            .with_result(
                "dpkg-query -W -f=${Version} codeterm",
                fail(1, "query failed"),
            )
            .with_result(
                "dpkg-query -W -f=${db:Status-Abbrev} ${Status} codeterm",
                fail(1, "query failed"),
            )
            .with_result("dpkg --audit", ok(""));
        let err =
            install_verified_deb_with(std::path::Path::new("/tmp/a.deb"), "1.7.1", &runner, false)
                .unwrap_err();
        assert!(err.contains("repair_required"));
        assert!(err.contains("installed_version=<none>"));
        assert!(err.contains("status=<unknown>"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn staging_runs_through_async_boundary() {
        let path = stage_deb_atomically_async(b"deb bytes".to_vec(), "9.9.9".to_string())
            .await
            .unwrap();
        assert!(path.ends_with("CodeTerm_9.9.9_amd64.deb"));
        let body = std::fs::read(&path).unwrap();
        assert_eq!(body, b"deb bytes");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn installer_runs_through_async_boundary() {
        struct ThreadSafeRunner;

        impl CommandRunner for ThreadSafeRunner {
            fn exists(&self, program: &str) -> bool {
                program == "sudo"
            }

            fn run(&self, program: &str, args: &[&str]) -> CommandResult {
                if program == "dpkg-query" && args.contains(&"-f=${Version}") {
                    return ok("1.7.1");
                }
                if program == "dpkg-query" {
                    return ok("ii install ok installed");
                }
                if program == "dpkg" {
                    return ok("");
                }
                assert_eq!(program, "sudo");
                assert!(args.contains(&"-n"));
                CommandResult {
                    success: true,
                    code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                }
            }
        }

        install_verified_deb_with_async(
            std::path::PathBuf::from("/tmp/CodeTerm_1.7.1_amd64.deb"),
            "1.7.1".to_string(),
            ThreadSafeRunner,
            false,
        )
        .await
        .unwrap();
    }

    #[test]
    fn appimage_is_not_debian() {
        assert_ne!(InstallMethod::AppImage, InstallMethod::Deb);
    }
}
