//! Hook-independent session binding. Once a hooked provider's launch has missed
//! its binding deadline, the provider reads its own record of this tab's launch.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::lock_hold::TimedMutex;

use super::ServiceContext;
use crate::agent::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};
use crate::session::lifecycle::{AgentTypeWriteOrigin, BindingSource};

static LAUNCH_MARKERS: LazyLock<TimedMutex<HashMap<String, String>>> =
    LazyLock::new(|| TimedMutex::new("launch_markers", HashMap::new()));
static IN_FLIGHT: LazyLock<TimedMutex<HashSet<String>>> =
    LazyLock::new(|| TimedMutex::new("launch_session_detection", HashSet::new()));

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

pub(crate) fn register_resume_launch_marker(tab_id: &str, session_id: &str) {
    LAUNCH_MARKERS
        .lock()
        .insert(tab_id.to_string(), session_id.to_string());
}

pub fn clear_launch_marker(tab_id: &str) {
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
        crate::agent::stub_launch::apply(
            provider_id,
            crate::service::tool_less_instructions::build(provider, params),
        )
    })
}

pub(crate) fn marked_provider_launch_command_for_service(
    service: &ServiceContext,
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
    params: &mut crate::agent::provider::AgentLaunchParams,
) -> Option<String> {
    super::directory_pre_trust::for_tab(service, registry, provider_id, tab_id, cwd);
    super::task_files::capture(service, tab_id, || {
        marked_provider_launch_command(
            registry,
            provider_id,
            tab_id,
            cwd,
            canonical_project,
            params,
        )
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
    let Some(flag) = registry
        .with_provider(provider_id, |provider| {
            provider.additional_writable_dir_flag()
        })
        .flatten()
    else {
        return;
    };
    match sandbox_agent_pad_root(
        registry,
        provider_id,
        skip_permissions,
        cwd,
        data_dir,
        tab_id,
    ) {
        Ok(root) => {
            let roots = combine_host_writable_roots(root, None);
            for root in &roots {
                args.extend([flag.clone(), root.to_string_lossy().into_owned()]);
            }
            log_sandbox_additional_roots_included(tab_id, roots.len());
        }
        Err(error) => {
            log::warn!("[sandbox-agent-pad] tab={tab_id} provider={provider_id} skipped: {error}")
        }
    }
}

fn sandbox_agent_pad_root(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    cwd: Option<&str>,
    data_dir: &std::path::Path,
    tab_id: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    if skip_permissions
        || !supports_additional_roots(registry, provider_id)
        || !sandbox_additional_writable_dirs_allowed(registry, provider_id, cwd, "agent_pad")
    {
        return Ok(None);
    }
    let pad = crate::service::agent_pad::restore(
        data_dir,
        tab_id,
        crate::service::agent_pad::AgentPadRole::General,
    )?;
    canonical_path_from_path(pad.path())
        .map(Some)
        .ok_or_else(|| "AgentPad path does not resolve".into())
}

fn supports_additional_roots(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) -> bool {
    registry
        .with_provider(provider_id, |provider| {
            provider.additional_writable_dir_flag().is_some()
        })
        .unwrap_or(false)
}

pub(crate) fn host_writable_roots(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
    data_dir: &std::path::Path,
    tab_id: &str,
) -> Result<Vec<std::path::PathBuf>, String> {
    Ok(combine_host_writable_roots(
        sandbox_agent_pad_root(
            registry,
            provider_id,
            skip_permissions,
            cwd,
            data_dir,
            tab_id,
        )?,
        sandbox_git_metadata_roots(
            registry,
            provider_id,
            skip_permissions,
            cwd,
            canonical_project,
        )?,
    ))
}

fn combine_host_writable_roots(
    pad: Option<std::path::PathBuf>,
    metadata: Option<[std::path::PathBuf; 4]>,
) -> Vec<std::path::PathBuf> {
    let mut seen = HashSet::new();
    pad.into_iter()
        .chain(metadata.into_iter().flatten())
        .filter(|root| seen.insert(root.clone()))
        .collect()
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
    let configured = configured_network_access(
        &capability.network_access_config_path,
        &capability.network_access_setting,
    );
    let access = sandbox_network_access_policy(
        skip_permissions,
        platform,
        !capability.network_access_args.is_empty(),
        configured,
    );
    let setting_override = format!("{}=true", capability.network_access_setting);
    if access != Some(true)
        || configured
        || capability.network_access_args.is_empty()
        || args.iter().any(|arg| arg == &setting_override)
    {
        return;
    }
    log::info!("[sandbox-network] granting Linux CodeTerm connectivity for provider={provider_id}");
    args.extend(capability.network_access_args);
}

pub(crate) fn host_network_access(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    platform: &str,
) -> Option<bool> {
    if skip_permissions || platform != "linux" {
        return None;
    }
    let capability = registry
        .with_provider(provider_id, |provider| {
            provider.sandboxed_linux_launch_capability()
        })
        .flatten()?;
    sandbox_network_access_policy(
        skip_permissions,
        platform,
        !capability.network_access_args.is_empty(),
        configured_network_access(
            &capability.network_access_config_path,
            &capability.network_access_setting,
        ),
    )
}

fn sandbox_network_access_policy(
    skip_permissions: bool,
    platform: &str,
    can_enable: bool,
    configured: bool,
) -> Option<bool> {
    (!skip_permissions && platform == "linux").then_some(can_enable || configured)
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
    let Some(flag) = registry
        .with_provider(provider_id, |provider| {
            provider.additional_writable_dir_flag()
        })
        .flatten()
    else {
        return;
    };
    match sandbox_git_metadata_roots(
        registry,
        provider_id,
        skip_permissions,
        cwd,
        canonical_project,
    ) {
        Ok(roots) => {
            let roots = combine_host_writable_roots(None, roots);
            for root in &roots {
                args.extend([flag.clone(), root.to_string_lossy().into_owned()]);
            }
            log_sandbox_additional_roots_included(tab_id, roots.len());
        }
        Err(error) => log::warn!("[sandbox-git-roots] refused provider={provider_id}: {error}"),
    }
}

fn sandbox_git_metadata_roots(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    skip_permissions: bool,
    cwd: Option<&str>,
    canonical_project: Option<&str>,
) -> Result<Option<[std::path::PathBuf; 4]>, String> {
    if skip_permissions
        || !supports_additional_roots(registry, provider_id)
        || !sandbox_additional_writable_dirs_allowed(registry, provider_id, cwd, "git_metadata")
    {
        return Ok(None);
    }
    match (cwd, canonical_project) {
        (Some(cwd), Some(project)) => git_worktree_metadata_dirs(cwd, project),
        _ => Ok(None),
    }
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
    selection: crate::ports::transport::Selection,
) -> Option<String> {
    if let Some(command) =
        super::codex_rpc::launch::selection_plan(svc, tab_id, provider_id, cwd, params, selection)
            .and_then(|plan| super::codex_rpc::launch::prepare_selection(svc, tab_id, plan))
    {
        return Some(command);
    }
    if let Err(error) = svc.ensure_agent_pad(tab_id) {
        log::warn!("[sandbox-agent-pad] tab={tab_id} has no AgentPad before launch: {error}");
    }
    let canonical_project = canonical_project_for_tab(&svc.plan_store, tab_id, cwd);
    marked_provider_launch_command_for_service(
        svc,
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

#[cfg(any(test, feature = "test-support"))]
pub fn registered_launch_marker(tab_id: &str) -> Option<String> {
    LAUNCH_MARKERS.lock().get(tab_id).cloned()
}

fn resumed_session_target<'a>(
    tab: &'a crate::multiplexer::types::Tab,
    marker: Option<&str>,
) -> Option<&'a str> {
    let target = tab.agent_session_id.as_deref().or_else(|| {
        tab.prior_session_binding_nonces
            .iter()
            .rev()
            .find(|entry| Some(entry.nonce.as_str()) == tab.session_binding_nonce.as_deref())
            .and_then(|entry| entry.resume_target_session_id.as_deref())
    });
    target.filter(|session| !session.is_empty() && marker == Some(*session))
}

fn resume_session_binding_allowed(session_exists: Option<bool>) -> bool {
    session_exists == Some(true)
}

fn provider_owns_resume_session(
    provider: &dyn crate::agent::provider::AgentProvider,
    cwd: &str,
    session_id: &str,
) -> bool {
    resume_session_binding_allowed(provider.session_exists(cwd, session_id))
}

impl ServiceContext {
    /// One detection per tab at a time. A live current-PTY provider record can
    /// re-authenticate a restored session after its launch nonce has rotated.
    pub(crate) fn detect_unbound_launch_session(&self, tab_id: &str) {
        if !IN_FLIGHT.lock().insert(tab_id.to_string()) {
            return;
        }
        let fallback_identity = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| Some((tab.agent_type.clone()?, tab.session_binding_nonce.clone()?)))
        }
        .filter(|(provider_id, _)| {
            !self.launch_has_resume_target(tab_id)
                && self
                    .agent_registry
                    .with_provider(provider_id, |provider| {
                        !provider.has_hooks() && provider.supports_launch_session_detection()
                    })
                    .unwrap_or(false)
        });
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        crate::async_runtime::spawn(async move {
            let probe_svc = svc.clone();
            let probe_tab = tab_id.clone();
            let Ok(detected) =
                tokio::task::spawn_blocking(move || probe_svc.probe_launch_session(&probe_tab))
                    .await
            else {
                IN_FLIGHT.lock().remove(&tab_id);
                return;
            };
            if let Some(detected) = detected {
                svc.commit_detected_launch_session(&tab_id, detected).await;
            } else if let Some((provider_id, nonce)) = fallback_identity {
                let fallback_svc = svc.clone();
                let fallback_tab = tab_id.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let still_current = {
                        let mux = fallback_svc.mux.read();
                        mux.get_tab(&fallback_tab).is_some_and(|tab| {
                            tab.agent_type.as_deref() == Some(provider_id.as_str())
                                && tab.session_binding_nonce.as_deref() == Some(nonce.as_str())
                        })
                    };
                    if still_current {
                        fallback_svc.detect_agent_session_with_fallback(
                            &fallback_tab,
                            &provider_id,
                            true,
                        );
                    }
                })
                .await;
            }
            IN_FLIGHT.lock().remove(&tab_id);
        });
    }

    pub(crate) fn launch_has_resume_target(&self, tab_id: &str) -> bool {
        let mux = self.mux.read();
        mux.get_tab(tab_id).is_some_and(|tab| {
            resumed_session_target(tab, LAUNCH_MARKERS.lock().get(tab_id).map(String::as_str))
                .is_some()
        })
    }

    pub(crate) fn resumed_provider_is_live(&self, tab_id: &str, pty_id: &str) -> bool {
        let current = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).is_some_and(|tab| {
                tab.pty_id.as_deref() == Some(pty_id)
                    && resumed_session_target(
                        tab,
                        LAUNCH_MARKERS.lock().get(tab_id).map(String::as_str),
                    )
                    .is_some()
            })
        };
        current
            && self
                .observe_provider_process_tree(tab_id, pty_id)
                .is_some_and(|tree| tree.state() == super::process_tree::ProviderTreeState::Present)
    }

    pub(crate) fn confirm_resumed_launch_session(&self, tab_id: &str) -> bool {
        if let Some(detected) = self.probe_resumed_launch_session(tab_id) {
            crate::async_runtime::block_on(self.commit_detected_launch_session(tab_id, detected));
        }
        self.mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.launch_session_authenticated())
    }

    fn probe_resumed_launch_session(&self, tab_id: &str) -> Option<DetectedLaunchSession> {
        let (provider_id, pty_id, binding_nonce, cwd, session_id) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.launch_session_authenticated() || tab.fork_source_session_id.is_some() {
                return None;
            }
            let marker = LAUNCH_MARKERS.lock().get(tab_id).cloned();
            let session_id = resumed_session_target(tab, marker.as_deref())?;
            (
                tab.agent_type.clone()?,
                tab.pty_id.clone()?,
                tab.session_binding_nonce.clone()?,
                tab.cwd.clone().unwrap_or_default(),
                session_id.to_string(),
            )
        };
        if !self.resumed_provider_is_live(tab_id, &pty_id) {
            return None;
        }
        let owns_session = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider_owns_resume_session(provider, &cwd, &session_id)
            })
            .unwrap_or(false);
        log::debug!("[resume-session-evidence] tab={tab_id} session={session_id} owns_session={owns_session}");
        owns_session.then_some(DetectedLaunchSession {
            provider_id,
            binding_nonce,
            detection: LaunchSessionDetection {
                session_id,
                source: LaunchSessionSource::LaunchMarker,
            },
        })
    }

    fn probe_launch_session(&self, tab_id: &str) -> Option<DetectedLaunchSession> {
        if self.launch_has_resume_target(tab_id) {
            return self.probe_resumed_launch_session(tab_id);
        }
        let (provider_id, pty_id, binding_nonce, cwd, launched_at_ms) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.launch_session_authenticated() {
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
        let claim = match crate::service::hook_session_binding::bind_detected_session(
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
        if let Err((status, message)) =
            crate::service::hook_session_binding::finish_session_binding(
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
        let mux = self.mux.read();
        if mux.get_tab(tab_id).is_some_and(|tab| {
            tab.session_binding_nonce.as_deref() == Some(binding_nonce.as_str())
                && tab.agent_session_id.as_deref() == Some(session_id.as_str())
                && tab.launch_session_authenticated()
        }) {
            LAUNCH_MARKERS.lock().remove(tab_id);
        }
    }
}

#[cfg(test)]
mod tests;
