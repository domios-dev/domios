//! Hook-independent session binding. Once a hooked provider's launch has missed
//! its binding deadline, the provider reads its own record of this tab's launch.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};
use crate::session::lifecycle::{AgentTypeWriteOrigin, BindingSource};

static LAUNCH_MARKERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

struct DetectedLaunchSession {
    provider_id: String,
    binding_nonce: String,
    detection: LaunchSessionDetection,
}

pub(crate) fn binding_source(source: LaunchSessionSource) -> BindingSource {
    match source {
        LaunchSessionSource::PidRegistry => BindingSource::PidRegistry,
        LaunchSessionSource::LaunchMarker => BindingSource::LaunchMarker,
    }
}

fn source_label(source: BindingSource) -> &'static str {
    match source {
        BindingSource::Hook => "hook",
        BindingSource::PidRegistry => "pid_registry",
        BindingSource::LaunchMarker => "launch_marker",
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn mint_launch_marker(tab_id: &str) -> String {
    let marker = format!("ct-launch-{}", uuid::Uuid::new_v4().simple());
    LAUNCH_MARKERS
        .lock()
        .insert(tab_id.to_string(), marker.clone());
    marker
}

pub(crate) fn clear_launch_marker(tab_id: &str) {
    LAUNCH_MARKERS.lock().remove(tab_id);
}

/// A launch command carrying the inert marker this launch registers for its
/// tab. Minted for every provider; the provider alone decides whether to use it.
pub(crate) fn marked_agent_command(
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &crate::agent::registry::AgentRegistry,
) -> String {
    marked_agent_command_inner(
        None,
        tab_id,
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
    )
}

pub(crate) fn marked_agent_command_for_service(
    service: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &crate::agent::registry::AgentRegistry,
) -> String {
    marked_agent_command_inner(
        Some(service),
        tab_id,
        agent_cmd,
        params,
        task,
        injected_session_id,
        registry,
    )
}

fn marked_agent_command_inner(
    service: Option<&ServiceContext>,
    tab_id: &str,
    agent_cmd: &str,
    params: &super::SpawnAgentParams,
    task: Option<&str>,
    injected_session_id: Option<&str>,
    registry: &crate::agent::registry::AgentRegistry,
) -> String {
    let marker = mint_launch_marker(tab_id);
    let command = match service {
        Some(service) => super::orchestrator::build_agent_command_with_launch_marker_for_service(
            service,
            tab_id,
            agent_cmd,
            params,
            task,
            injected_session_id,
            registry,
            Some(&marker),
        ),
        None => super::orchestrator::build_agent_command_with_launch_marker(
            agent_cmd,
            params,
            task,
            injected_session_id,
            registry,
            Some(&marker),
        ),
    };
    if command.trim().is_empty() {
        clear_launch_marker(tab_id);
    }
    command
}

/// Gives hand-built fresh launch params a marker minted and registered for this tab.
pub(crate) fn mark_fresh_launch(
    tab_id: &str,
    params: &mut crate::agent::provider::AgentLaunchParams,
) {
    params.launch_marker = Some(mint_launch_marker(tab_id));
}

/// The provider's stub-aware fresh launch command for hand-built params,
/// carrying this tab's marker.
pub(crate) fn marked_provider_launch_command(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
    params: &mut crate::agent::provider::AgentLaunchParams,
) -> Option<String> {
    mark_fresh_launch(tab_id, params);
    let mut args = params.args.clone().unwrap_or_default();
    append_sandbox_linux_network_args(
        registry,
        provider_id,
        params.skip_permissions,
        crate::agent::provider::current_platform_tag(),
        &mut args,
    );
    append_sandbox_git_metadata_args(
        registry,
        provider_id,
        params.skip_permissions,
        cwd,
        canonical_project,
        tab_id,
        &mut args,
    );
    append_sandbox_agent_pad_arg(
        registry,
        provider_id,
        params.skip_permissions,
        cwd,
        &crate::paths::data_dir(),
        tab_id,
        &mut args,
    );
    params.args = (!args.is_empty()).then_some(args);
    registry.with_provider(provider_id, |provider| {
        crate::agent::stub_launch::apply(provider_id, provider.build_launch_command(params))
    })
}

/// A sandboxed agent still writes its report through its own AgentPad.
pub(crate) fn append_sandbox_agent_pad_arg(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    cwd: Option<&str>,
    data_dir: &std::path::Path,
    tab_id: &str,
    args: &mut Vec<String>,
) {
    if skip_permissions {
        return;
    }
    let Some(flag) = registry
        .with_provider(provider_id, |provider| {
            provider.additional_writable_dir_flag()
        })
        .flatten()
    else {
        return;
    };
    if !sandbox_additional_writable_dirs_allowed(registry, provider_id, cwd, "agent_pad") {
        return;
    }
    let pad = match crate::service::agent_pad::restore(
        data_dir,
        tab_id,
        crate::service::agent_pad::AgentPadRole::General,
    ) {
        Ok(pad) => pad,
        Err(error) => {
            log::warn!("[sandbox-agent-pad] tab={tab_id} provider={provider_id} skipped: {error}");
            return;
        }
    };
    let Some(path) = canonical_path_from_path(pad.path()) else {
        log::warn!("[sandbox-agent-pad] tab={tab_id} provider={provider_id} skipped: pad path does not resolve");
        return;
    };
    args.push(flag);
    args.push(path.to_string_lossy().into_owned());
    log_sandbox_additional_roots_included(tab_id, 1);
}

pub(crate) fn append_sandbox_linux_network_args(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    platform: &str,
    args: &mut Vec<String>,
) {
    if skip_permissions || platform != "linux" {
        return;
    }
    let Some(capability) = registry
        .with_provider(provider_id, |provider| {
            provider.sandboxed_linux_launch_capability()
        })
        .flatten()
    else {
        return;
    };
    let setting_override = format!("{}=true", capability.network_access_setting);
    if capability.network_access_args.is_empty()
        || configured_network_access(
            &capability.network_access_config_path,
            &capability.network_access_setting,
        )
        || args.iter().any(|arg| arg == &setting_override)
    {
        return;
    }
    log::info!("[sandbox-network] granting Linux CodeTerm connectivity for provider={provider_id}");
    args.extend(capability.network_access_args);
}

fn configured_network_access(path: &str, setting: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let Some(path) = path.strip_prefix("~/").map(|relative| home.join(relative)) else {
        return false;
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    toml_setting_is_true(&contents, setting)
}

fn toml_setting_is_true(contents: &str, setting: &str) -> bool {
    let Ok(config) = contents.parse::<toml::Value>() else {
        return false;
    };
    setting
        .split('.')
        .try_fold(&config, |value, key| value.get(key))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

#[cfg(test)]
mod sandbox_network_tests {
    use super::toml_setting_is_true;

    #[test]
    fn existing_network_access_setting_is_detected_without_rewriting_config() {
        assert!(toml_setting_is_true(
            "[sandbox_workspace_write]\nnetwork_access = true\n",
            "sandbox_workspace_write.network_access"
        ));
        assert!(!toml_setting_is_true(
            "[sandbox_workspace_write]\nnetwork_access = false\n",
            "sandbox_workspace_write.network_access"
        ));
    }
}

pub(crate) fn append_sandbox_git_metadata_args(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
    tab_id: &str,
    args: &mut Vec<String>,
) {
    if skip_permissions || canonical_project.is_none() {
        return;
    }
    let Some(flag) = registry
        .with_provider(provider_id, |provider| {
            provider.additional_writable_dir_flag()
        })
        .flatten()
    else {
        return;
    };
    if !sandbox_additional_writable_dirs_allowed(registry, provider_id, cwd, "git_metadata") {
        return;
    }
    let Some(cwd) = cwd else { return };
    let canonical_project = canonical_project.expect("checked above");
    let paths = match git_worktree_metadata_dirs(cwd, canonical_project) {
        Ok(Some(paths)) => paths,
        Ok(None) => return,
        Err(error) => {
            log::warn!("[sandbox-git-roots] refused provider={provider_id}: {error}");
            return;
        }
    };
    let root_count = paths.len();
    for path in paths {
        args.push(flag.clone());
        args.push(path.to_string_lossy().into_owned());
    }
    log_sandbox_additional_roots_included(tab_id, root_count);
}

fn sandbox_additional_roots_log(tab_id: &str, root_count: usize) -> Option<String> {
    (root_count > 0)
        .then(|| format!("[sandbox-additional-roots] included {root_count} roots for tab {tab_id}"))
}

fn log_sandbox_additional_roots_included(tab_id: &str, root_count: usize) {
    if let Some(message) = sandbox_additional_roots_log(tab_id, root_count) {
        log::info!("{message}");
    }
}

fn sandbox_additional_writable_dirs_allowed(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    cwd: Option<&str>,
    source: &str,
) -> bool {
    let trust_state = registry
        .with_provider(provider_id, |provider| {
            // Providers key trust by plain paths; a verbatim-prefixed cwd would match no entry.
            provider.directory_trust_state(
                &codeterm_paths::strip_verbatim_prefix(cwd.unwrap_or_default()),
                false,
            )
        })
        .flatten();
    match trust_state {
        None | Some(codeterm_plugin::provider::DirectoryTrustState::Trusted) => true,
        Some(state) => {
            log::info!(
                "[sandbox-additional-roots] provider={provider_id} source={source} omitted: directory trust state is {state:?}"
            );
            false
        }
    }
}

pub(crate) fn canonical_project_for_task_lane(
    plan_store: &crate::service::plan::PlanStore,
    plan_id: &str,
    task_id: &str,
    cwd: &str,
) -> Option<String> {
    let plan = plan_store.get(plan_id)?;
    let task = plan.tasks.iter().find(|task| task.id == task_id)?;
    let lane = task.lane.worktree_path.as_deref()?;
    if canonical_path(lane)? != canonical_path(cwd)? {
        return None;
    }
    let project = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    };
    let project = canonical_path(project)?;
    Some(project.to_string_lossy().into_owned())
}

pub(crate) fn canonical_project_for_launch(
    service: &ServiceContext,
    tab_id: &str,
    params: &super::orchestrator::SpawnAgentParams,
    cwd: &str,
) -> Option<String> {
    if let Some(authority) = params.task_launch_authority.as_ref() {
        if params.plan_id.as_deref() != Some(authority.plan_id.as_str())
            || params.task_id.as_deref() != Some(authority.task_id.as_str())
        {
            return None;
        }
        return canonical_project_for_task_lane(
            &service.plan_store,
            &authority.plan_id,
            &authority.task_id,
            cwd,
        );
    }
    let (plan_id, task_id) = service.plan_store.task_binding_for_tab(tab_id)?;
    canonical_project_for_task_lane(&service.plan_store, &plan_id, &task_id, cwd)
}

pub(crate) fn marked_launch_command_for_tab(
    svc: &crate::service::ServiceContext,
    provider_id: &str,
    tab_id: &str,
    cwd: &str,
    params: &mut crate::agent::provider::AgentLaunchParams,
) -> Option<String> {
    if let Err(error) = svc.ensure_agent_pad(tab_id) {
        log::warn!("[sandbox-agent-pad] tab={tab_id} has no AgentPad before launch: {error}");
    }
    let canonical_project = canonical_project_for_tab(&svc.plan_store, tab_id, cwd);
    marked_provider_launch_command(
        &svc.agent_registry,
        provider_id,
        tab_id,
        Some(cwd),
        canonical_project.as_deref(),
        params,
    )
}

pub(crate) fn canonical_project_for_tab(
    plan_store: &crate::service::plan::PlanStore,
    tab_id: &str,
    cwd: &str,
) -> Option<String> {
    let (plan_id, task_id) = plan_store.task_binding_for_tab(tab_id)?;
    canonical_project_for_task_lane(plan_store, &plan_id, &task_id, cwd)
}

fn git_worktree_metadata_dirs(
    cwd: &str,
    canonical_project: &str,
) -> Result<Option<[std::path::PathBuf; 4]>, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use wait_timeout::ChildExt;

    let cwd = canonical_path(cwd).ok_or_else(|| "lane cwd cannot be canonicalized".to_string())?;
    let project = canonical_path(canonical_project)
        .ok_or_else(|| "canonical project cannot be canonicalized".to_string())?;
    if !cwd.join(".git").is_file() {
        return Ok(None);
    }
    let common_expected = canonical_path_from_path(&project.join(".git"))
        .ok_or_else(|| "canonical project .git directory is unavailable".to_string())?;
    let project_git_metadata = std::fs::symlink_metadata(project.join(".git"))
        .map_err(|error| format!("canonical project .git is unavailable: {error}"))?;
    if !project_git_metadata.is_dir() || project_git_metadata.file_type().is_symlink() {
        return Err("canonical project .git is not a directory".into());
    }

    let cwd_text = cwd.to_string_lossy().into_owned();
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    clear_git_environment(&mut command);
    let mut child = command
        .current_dir(&cwd_text)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git rev-parse could not start: {error}"))?;
    let status = match child.wait_timeout(Duration::from_secs(3)) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("git rev-parse exceeded the 3 second deadline".into());
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("git rev-parse wait failed: {error}"));
        }
    };
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout)
            .map_err(|error| format!("git rev-parse stdout failed: {error}"))?;
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr)
            .map_err(|error| format!("git rev-parse stderr failed: {error}"))?;
    }
    if !status.success() {
        return Err(format!("git rev-parse failed: {}", stderr.trim()));
    }
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let (Some(git_dir), Some(common_dir)) = (lines.next(), lines.next()) else {
        return Err("git rev-parse returned incomplete paths".into());
    };
    let git_dir =
        canonical_path(git_dir).ok_or_else(|| "lane gitdir cannot be canonicalized".to_string())?;
    let common_dir = canonical_path(common_dir)
        .ok_or_else(|| "git common dir cannot be canonicalized".to_string())?;
    if git_dir == common_dir {
        return Ok(None);
    }
    if common_dir != common_expected {
        return Err("git common dir does not belong to the canonical project".into());
    }
    let worktrees_dir = canonical_path_from_path(&common_dir.join("worktrees"))
        .ok_or_else(|| "canonical project worktrees directory is unavailable".to_string())?;
    if worktrees_dir.parent() != Some(common_dir.as_path())
        || git_dir.parent() != Some(worktrees_dir.as_path())
    {
        return Err("lane gitdir is outside the canonical project worktrees directory".into());
    }
    validate_worktree_pointer(&cwd, &git_dir, &common_dir)?;

    let roots = [
        git_dir,
        canonical_path_from_path(&common_dir.join("objects"))
            .ok_or_else(|| "common objects directory is unavailable".to_string())?,
        canonical_path_from_path(&common_dir.join("refs"))
            .ok_or_else(|| "common refs directory is unavailable".to_string())?,
        canonical_path_from_path(&common_dir.join("logs"))
            .ok_or_else(|| "common logs directory is unavailable".to_string())?,
    ];
    if roots[1..]
        .iter()
        .any(|path| path.parent() != Some(common_dir.as_path()) || !path.is_dir())
    {
        return Err("common metadata roots are not direct project git directories".into());
    }
    Ok(Some(roots))
}

fn canonical_path(path: &str) -> Option<std::path::PathBuf> {
    let path = codeterm_paths::strip_verbatim_prefix(path);
    canonical_path_from_path(std::path::Path::new(&path))
}

fn canonical_path_from_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let resolved = path.canonicalize().ok()?;
    let clean = codeterm_paths::strip_verbatim_prefix(&resolved.to_string_lossy());
    Some(std::path::PathBuf::from(clean))
}

fn clear_git_environment(command: &mut std::process::Command) {
    let git_env = std::env::vars_os()
        .filter_map(|(key, _)| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
                .then_some(key)
        })
        .collect::<Vec<_>>();
    for key in git_env {
        command.env_remove(key);
    }
}

fn validate_worktree_pointer(
    cwd: &std::path::Path,
    git_dir: &std::path::Path,
    common_dir: &std::path::Path,
) -> Result<(), String> {
    let commondir = std::fs::read_to_string(git_dir.join("commondir"))
        .map_err(|error| format!("lane gitdir commondir is unavailable: {error}"))?;
    let common_target = cwd_relative_metadata_path(git_dir, commondir.trim())
        .ok_or_else(|| "lane gitdir commondir target cannot be resolved".to_string())?;
    if common_target != common_dir {
        return Err("lane gitdir commondir points outside the canonical project".into());
    }
    let gitdir_file = std::fs::read_to_string(git_dir.join("gitdir"))
        .map_err(|error| format!("lane gitdir pointer is unavailable: {error}"))?;
    let worktree_dot_git = cwd.join(".git");
    let gitdir_target = cwd_relative_metadata_path(git_dir, gitdir_file.trim())
        .ok_or_else(|| "lane gitdir pointer target cannot be resolved".to_string())?;
    if gitdir_target != canonical_path_from_path(&worktree_dot_git).unwrap_or_default() {
        return Err("lane gitdir pointer does not name this worktree".into());
    }
    Ok(())
}

fn cwd_relative_metadata_path(base: &std::path::Path, value: &str) -> Option<std::path::PathBuf> {
    let value = codeterm_paths::strip_verbatim_prefix(value);
    let path = std::path::Path::new(&value);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    canonical_path_from_path(&path)
}

#[cfg(test)]
pub(crate) fn registered_launch_marker(tab_id: &str) -> Option<String> {
    LAUNCH_MARKERS.lock().get(tab_id).cloned()
}

impl ServiceContext {
    /// One detection per tab at a time. Detection itself refuses a bound tab,
    /// and the commit refuses a launch the tab has since rotated.
    pub(crate) fn detect_unbound_launch_session(&self, tab_id: &str) {
        if !IN_FLIGHT.lock().insert(tab_id.to_string()) {
            return;
        }
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        tauri::async_runtime::spawn(async move {
            let probe_svc = svc.clone();
            let probe_tab = tab_id.clone();
            let detected =
                tokio::task::spawn_blocking(move || probe_svc.probe_launch_session(&probe_tab))
                    .await
                    .ok()
                    .flatten();
            if let Some(detected) = detected {
                svc.commit_detected_launch_session(&tab_id, detected).await;
            }
            IN_FLIGHT.lock().remove(&tab_id);
        });
    }

    fn probe_launch_session(&self, tab_id: &str) -> Option<DetectedLaunchSession> {
        let (provider_id, pty_id, binding_nonce, cwd, launched_at_ms) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.agent_session_id.is_some() {
                return None;
            }
            let launched_at_ms = tab
                .agent_set_at
                .map(|set_at| epoch_ms().saturating_sub(set_at.elapsed().as_millis() as u64))
                .unwrap_or(0);
            (
                tab.agent_type.clone()?,
                tab.pty_id.clone()?,
                tab.session_binding_nonce.clone()?,
                tab.cwd.clone().unwrap_or_default(),
                launched_at_ms,
            )
        };
        let root_pid = {
            let pty = self.pty.lock();
            let session = pty.get_session(&pty_id)?;
            let pid = session.child.lock().ok()?.process_id();
            pid?
        };
        let evidence = LaunchSessionEvidence {
            cwd,
            launch_marker: LAUNCH_MARKERS.lock().get(tab_id).cloned(),
            launched_at_ms,
            processes: super::process_tree::launch_processes(root_pid),
        };
        let detection = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider.detect_launch_session(&evidence)
            })
            .flatten()?;
        Some(DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection,
        })
    }

    async fn commit_detected_launch_session(&self, tab_id: &str, detected: DetectedLaunchSession) {
        let DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection,
        } = detected;
        let source = binding_source(detection.source);
        let session_id = detection.session_id;
        let claim = match crate::api::hook_routes::bind_detected_session(
            &self.mux,
            self.agent_registry.as_ref(),
            tab_id,
            &provider_id,
            &session_id,
            &binding_nonce,
        ) {
            Ok(claim) => claim,
            Err((status, message, _)) => {
                log::info!(
                    "[launch-session-detection] Tab {tab_id} refused {provider_id} session {session_id} from {} ({status}): {message}",
                    source_label(source),
                );
                return;
            }
        };
        log::info!(
            "[launch-session-detection] Tab {tab_id} bound {provider_id} session {session_id} from {}",
            source_label(source),
        );
        LAUNCH_MARKERS.lock().remove(tab_id);
        if let Err((status, message)) = crate::api::hook_routes::finish_session_binding(
            self,
            tab_id,
            &provider_id,
            &session_id,
            Some(&binding_nonce),
            Some(AgentTypeWriteOrigin::Hook),
            source_label(source),
            &serde_json::Value::Null,
            claim,
            source,
            false,
        )
        .await
        {
            log::warn!(
                "[launch-session-detection] Tab {tab_id} binding follow-up failed ({status}): {message}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(cwd: &std::path::Path, args: &[&str]) {
        let mut command = std::process::Command::new("git");
        crate::process_util::no_window_std(&mut command);
        clear_git_environment(&mut command);
        let output = command
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn lane_git_writable_roots_derived_from_real_worktree() {
        let temp = tempfile::tempdir().expect("temporary repository");
        let repo = temp.path().join("repo");
        let lane = temp.path().join("lane");
        std::fs::create_dir(&repo).expect("repository directory");
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=CodeTerm",
                "-c",
                "user.email=codeterm@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "lane/worker",
                lane.to_str().unwrap(),
            ],
        );

        let roots = git_worktree_metadata_dirs(lane.to_str().unwrap(), repo.to_str().unwrap())
            .expect("valid lane metadata")
            .expect("linked worktree");
        let mut command = std::process::Command::new("git");
        crate::process_util::no_window_std(&mut command);
        clear_git_environment(&mut command);
        let output = command
            .current_dir(&lane)
            .args(["rev-parse", "--git-dir"])
            .output()
            .expect("git reports lane gitdir");
        let lane_gitdir = String::from_utf8(output.stdout).expect("path is UTF-8");
        let lane_gitdir = std::path::Path::new(lane_gitdir.trim());
        let lane_gitdir = if lane_gitdir.is_absolute() {
            lane_gitdir.to_path_buf()
        } else {
            lane.join(lane_gitdir)
        };
        let expected_lane_gitdir =
            canonical_path_from_path(&lane_gitdir).expect("lane gitdir resolves");
        assert_eq!(roots[0], expected_lane_gitdir);
        assert!(roots[0].join("commondir").is_file());
        assert_ne!(roots[0], repo);
        let common = canonical_path_from_path(&repo.join(".git")).expect("common directory");
        let worktrees = common.join("worktrees");
        assert_eq!(roots[0].parent(), Some(worktrees.as_path()));
        assert_eq!(roots[1], common.join("objects"));
        assert_eq!(roots[2], common.join("refs"));
        assert_eq!(roots[3], common.join("logs"));
    }

    #[test]
    fn crafted_gitfile_pointing_at_another_lane_is_refused() {
        let temp = tempfile::tempdir().expect("temporary repository");
        let repo = temp.path().join("repo");
        let lane = temp.path().join("lane");
        let forged = temp.path().join("forged-lane");
        std::fs::create_dir(&repo).expect("repository directory");
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=CodeTerm",
                "-c",
                "user.email=codeterm@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "lane/worker",
                lane.to_str().unwrap(),
            ],
        );
        std::fs::create_dir(&forged).expect("forged worktree directory");
        std::fs::write(
            forged.join(".git"),
            std::fs::read_to_string(lane.join(".git")).expect("lane gitfile"),
        )
        .expect("crafted gitfile");

        assert!(
            git_worktree_metadata_dirs(forged.to_str().unwrap(), repo.to_str().unwrap(),).is_err()
        );
    }

    #[test]
    fn ordinary_repository_has_no_additional_git_metadata_roots() {
        let temp = tempfile::tempdir().expect("temporary repository");
        git(temp.path(), &["init"]);
        assert!(git_worktree_metadata_dirs(
            temp.path().to_str().unwrap(),
            temp.path().to_str().unwrap(),
        )
        .expect("valid ordinary repository")
        .is_none());
    }

    #[test]
    fn sandboxed_launch_grants_only_its_own_agent_pad() {
        let data = tempfile::tempdir().expect("data directory");
        let pad = crate::service::agent_pad::AgentPad::create(
            data.path(),
            "tab-pad-1",
            crate::service::agent_pad::AgentPadRole::Worker,
        )
        .expect("pad");
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture")
                .with_additional_writable_dir_flag("--add-dir"),
        ));
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "no-flag",
        )));

        let mut sandboxed = Vec::new();
        append_sandbox_agent_pad_arg(
            &registry,
            "fixture",
            false,
            Some(data.path().to_str().unwrap()),
            data.path(),
            "tab-pad-1",
            &mut sandboxed,
        );
        let expected = canonical_path_from_path(pad.path()).expect("pad resolves");
        assert_eq!(
            sandboxed,
            vec![
                "--add-dir".to_string(),
                expected.to_string_lossy().into_owned()
            ]
        );

        let mut full_access = Vec::new();
        append_sandbox_agent_pad_arg(
            &registry,
            "fixture",
            true,
            Some(data.path().to_str().unwrap()),
            data.path(),
            "tab-pad-1",
            &mut full_access,
        );
        assert!(full_access.is_empty());
        let mut undeclared = Vec::new();
        append_sandbox_agent_pad_arg(
            &registry,
            "no-flag",
            false,
            Some(data.path().to_str().unwrap()),
            data.path(),
            "tab-pad-1",
            &mut undeclared,
        );
        assert!(undeclared.is_empty());
        let mut created = Vec::new();
        append_sandbox_agent_pad_arg(
            &registry,
            "fixture",
            false,
            Some(data.path().to_str().unwrap()),
            data.path(),
            "tab-without-pad",
            &mut created,
        );
        let created_pad = crate::service::agent_pad::AgentPad::open(data.path(), "tab-without-pad")
            .expect("launch creates the missing pad");
        let created_path = canonical_path_from_path(created_pad.path()).expect("pad resolves");
        assert_eq!(
            created,
            vec![
                "--add-dir".to_string(),
                created_path.to_string_lossy().into_owned()
            ]
        );
    }

    #[test]
    fn sandboxed_lane_launch_includes_git_roots_only_for_lanes() {
        let temp = tempfile::tempdir().expect("temporary repository");
        let repo = temp.path().join("repo");
        let lane = temp.path().join("lane");
        std::fs::create_dir(&repo).expect("repository directory");
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=CodeTerm",
                "-c",
                "user.email=codeterm@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "lane/worker",
                lane.to_str().unwrap(),
            ],
        );
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture")
                .with_additional_writable_dir_flag("--add-dir"),
        ));

        let roots = git_worktree_metadata_dirs(lane.to_str().unwrap(), repo.to_str().unwrap())
            .expect("valid lane metadata")
            .expect("linked worktree");
        let mut args = Vec::new();
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            false,
            Some(lane.to_str().unwrap()),
            Some(repo.to_str().unwrap()),
            "metadata-test-tab",
            &mut args,
        );
        assert_eq!(
            args,
            vec![
                "--add-dir".to_string(),
                roots[0].to_string_lossy().into_owned(),
                "--add-dir".to_string(),
                roots[1].to_string_lossy().into_owned(),
                "--add-dir".to_string(),
                roots[2].to_string_lossy().into_owned(),
                "--add-dir".to_string(),
                roots[3].to_string_lossy().into_owned(),
            ]
        );
        assert!(args.iter().all(|arg| arg != repo.to_str().unwrap()));
        assert!(args.iter().all(|arg| arg != lane.to_str().unwrap()));
        assert!(args
            .iter()
            .all(|arg| arg != repo.join(".git").to_str().unwrap()));

        let mut full_access_args = vec!["--existing".to_string()];
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            true,
            Some(lane.to_str().unwrap()),
            Some(repo.to_str().unwrap()),
            "metadata-test-tab",
            &mut full_access_args,
        );
        assert_eq!(full_access_args, vec!["--existing".to_string()]);

        let ordinary = temp.path().join("ordinary");
        std::fs::create_dir(&ordinary).expect("ordinary repository directory");
        git(&ordinary, &["init"]);
        let mut ordinary_args = vec!["--existing".to_string()];
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            false,
            Some(ordinary.to_str().unwrap()),
            Some(ordinary.to_str().unwrap()),
            "metadata-test-tab",
            &mut ordinary_args,
        );
        assert_eq!(ordinary_args, vec!["--existing".to_string()]);
    }

    #[test]
    fn a_verbatim_cwd_resolves_the_same_directory_trust_as_its_plain_spelling() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture")
                .with_trusted_directory("C:\\repo"),
        ));
        assert!(sandbox_additional_writable_dirs_allowed(
            &registry,
            "fixture",
            Some("\\\\?\\C:\\repo"),
            "agent_pad",
        ));
        assert!(!sandbox_additional_writable_dirs_allowed(
            &registry,
            "fixture",
            Some("C:\\other"),
            "agent_pad",
        ));
    }

    #[test]
    fn sandboxed_additional_roots_follow_provider_directory_trust_state() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let data = temp.path().join("data");
        std::fs::create_dir(&data).expect("data directory");
        let pad = crate::service::agent_pad::AgentPad::create(
            &data,
            "trust-state-tab",
            crate::service::agent_pad::AgentPadRole::General,
        )
        .expect("pad");
        let repo = temp.path().join("repo");
        let lane = temp.path().join("lane");
        std::fs::create_dir(&repo).expect("repository directory");
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=CodeTerm",
                "-c",
                "user.email=codeterm@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "lane/trust-state",
                lane.to_str().unwrap(),
            ],
        );
        let git_roots = git_worktree_metadata_dirs(lane.to_str().unwrap(), repo.to_str().unwrap())
            .expect("valid lane metadata")
            .expect("linked worktree");

        for (state, expected_count) in [
            (codeterm_plugin::provider::DirectoryTrustState::Trusted, 10),
            (codeterm_plugin::provider::DirectoryTrustState::Untrusted, 0),
            (codeterm_plugin::provider::DirectoryTrustState::Unknown, 0),
        ] {
            let registry = crate::agent::registry::AgentRegistry::new();
            registry.register_provider(Box::new(
                crate::agent::test_provider::MockProvider::new("fixture")
                    .with_additional_writable_dir_flag("--add-dir")
                    .with_directory_trust_state(state),
            ));
            let mut args = Vec::new();
            append_sandbox_agent_pad_arg(
                &registry,
                "fixture",
                false,
                Some(lane.to_str().unwrap()),
                &data,
                "trust-state-tab",
                &mut args,
            );
            append_sandbox_git_metadata_args(
                &registry,
                "fixture",
                false,
                Some(lane.to_str().unwrap()),
                Some(repo.to_str().unwrap()),
                "trust-state-tab",
                &mut args,
            );
            assert_eq!(args.len(), expected_count);
            if state == codeterm_plugin::provider::DirectoryTrustState::Trusted {
                let expected_pad = canonical_path_from_path(pad.path()).expect("pad resolves");
                assert_eq!(args[1], expected_pad.to_string_lossy());
                assert_eq!(args[3], git_roots[0].to_string_lossy());
                assert_eq!(args[5], git_roots[1].to_string_lossy());
            }
        }
    }

    #[test]
    fn detection_sources_journal_as_their_own_binding_source() {
        assert_eq!(
            binding_source(LaunchSessionSource::PidRegistry),
            BindingSource::PidRegistry
        );
        assert_eq!(
            binding_source(LaunchSessionSource::LaunchMarker),
            BindingSource::LaunchMarker
        );
        assert_ne!(
            binding_source(LaunchSessionSource::PidRegistry),
            BindingSource::Hook
        );
    }

    #[test]
    fn marked_agent_command_carries_the_marker_its_tab_currently_holds() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "marked",
        )));
        let params = crate::service::SpawnAgentParams {
            agent: Some("marked".into()),
            ..Default::default()
        };

        let first = marked_agent_command("tab-marked", "marked", &params, None, None, &registry);
        let first_marker = registered_launch_marker("tab-marked").expect("first marker");
        assert!(!first_marker.is_empty());
        assert_eq!(first, format!("marked-cli --launch-marker {first_marker}"));

        let relaunch = marked_agent_command("tab-marked", "marked", &params, None, None, &registry);
        let relaunch_marker = registered_launch_marker("tab-marked").expect("relaunch marker");
        assert_ne!(relaunch_marker, first_marker);
        assert_eq!(
            relaunch,
            format!("marked-cli --launch-marker {relaunch_marker}")
        );
    }

    #[test]
    fn marked_provider_launch_command_hands_every_fresh_launch_its_tabs_marker() {
        use crate::agent::provider::AgentLaunchParams;

        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "direct",
        )));
        let cases = [
            ("tab-direct-plain", AgentLaunchParams::default()),
            (
                "tab-direct-task",
                AgentLaunchParams {
                    task: Some("fixture task".into()),
                    ..Default::default()
                },
            ),
            (
                "tab-direct-injected",
                AgentLaunchParams {
                    session_id: Some("injected-session".into()),
                    ..Default::default()
                },
            ),
        ];

        for (tab_id, mut params) in cases {
            let command = marked_provider_launch_command(
                &registry,
                "direct",
                tab_id,
                None,
                None,
                &mut params,
            )
            .expect("registered provider builds a command");
            let marker = registered_launch_marker(tab_id).expect("tab registers its marker");
            assert_eq!(params.launch_marker.as_deref(), Some(marker.as_str()));
            assert_eq!(command, format!("direct-cli --launch-marker {marker}"));
        }
    }

    #[test]
    fn mark_fresh_launch_registers_the_marker_it_hands_the_params() {
        let mut params = crate::agent::provider::AgentLaunchParams::default();
        mark_fresh_launch("tab-mark-only", &mut params);
        let marker = registered_launch_marker("tab-mark-only").expect("registered marker");
        assert!(!marker.is_empty());
        assert_eq!(params.launch_marker, Some(marker));
    }

    #[test]
    fn sandbox_additional_roots_logged_when_included() {
        assert_eq!(
            sandbox_additional_roots_log("tab-1", 3).as_deref(),
            Some("[sandbox-additional-roots] included 3 roots for tab tab-1")
        );
        assert_eq!(sandbox_additional_roots_log("tab-1", 0), None);
    }
}
