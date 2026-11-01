use super::*;

pub(super) fn looks_like_local_path(value: &str) -> bool {
    value.starts_with("file://")
        || value.starts_with('.')
        || value.starts_with('/')
        || value.contains(":\\")
        || Path::new(value).exists()
}

pub(super) struct ChannelSource {
    pub(super) base: String,
    pub(super) git_ref: Option<String>,
    pub(super) subdir: Option<String>,
}

impl ChannelSource {
    pub(super) fn source_dir(&self) -> PathBuf {
        let base = local_path_from_url(&self.base).unwrap_or_else(|| PathBuf::from(&self.base));
        match &self.subdir {
            Some(subdir) => base.join(subdir),
            None => base,
        }
    }
}

pub(super) fn parse_channel_source(raw: &str) -> ChannelSource {
    let Some((base, fragment)) = raw.split_once('#') else {
        return ChannelSource {
            base: raw.to_string(),
            git_ref: None,
            subdir: None,
        };
    };
    let (git_ref, subdir) = fragment
        .split_once(':')
        .map(|(r, s)| (empty_to_none(r), empty_to_none(s)))
        .unwrap_or_else(|| (empty_to_none(fragment), None));
    ChannelSource {
        base: base.to_string(),
        git_ref,
        subdir,
    }
}

pub(super) fn empty_to_none(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) fn local_path_from_url(value: &str) -> Option<PathBuf> {
    let raw = value.strip_prefix("file://")?;
    #[cfg(target_os = "windows")]
    {
        let raw = raw.strip_prefix('/').unwrap_or(raw);
        return Some(PathBuf::from(raw));
    }
    #[cfg(not(target_os = "windows"))]
    {
        Some(PathBuf::from(raw))
    }
}

pub(super) fn clone_git_channel(source: &ChannelSource, cache_path: &Path) -> Result<(), String> {
    // Unique scratch dir per clone: a fixed "<cache>.git-clone" let racing clones of one
    // channel share a working tree and corrupt each other's git metadata.
    let tmp = cache_path.with_extension(format!("git-clone-{}", uuid::Uuid::new_v4()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("clear {}: {e}", tmp.display()))?;
    }
    let result = clone_git_channel_into(source, cache_path, &tmp);
    // Always drop the scratch clone (a no-op after rename-into-place) so failed clones
    // never accumulate under the channels dir.
    if tmp.exists() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

fn clone_git_channel_into(
    source: &ChannelSource,
    cache_path: &Path,
    tmp: &Path,
) -> Result<(), String> {
    if let Some(subdir) = &source.subdir {
        validate_relative_path(subdir)?;
    }
    let epoch = crate::git_probe::epoch();
    let mut commands =
        crate::git_tool::clone_commands(source, tmp, crate::git_tool::supports_sparse()?)
            .into_iter();
    if let Some(clone) = commands.next() {
        run_git_with(clone, None, epoch, git_command)?;
    }
    for command in commands {
        run_git_with(command, Some(tmp), epoch, git_command)?;
    }
    if let Some(subdir) = &source.subdir {
        copy_dir_filtered(&tmp.join(subdir), cache_path)?;
    } else {
        fs::rename(tmp, cache_path).map_err(|e| format!("rename channel cache: {e}"))?;
    }
    Ok(())
}

pub(super) fn safe_channel_name(value: &str) -> String {
    let leaf = Path::new(value)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(value);
    let out = leaf
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if out.is_empty() {
        "channel".to_string()
    } else {
        out
    }
}

pub(super) fn validate_relative_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() || path.contains("..") {
        return Err(format!("unsafe plugin path in channel: {path}"));
    }
    Ok(())
}

pub(super) fn absolutize(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|e| format!("resolve {}: {e}", path.display()))
}

#[cfg(all(test, unix))]
pub(super) fn run_git(args: Vec<String>) -> Result<(), String> {
    run_git_with(args, None, crate::git_probe::epoch(), git_command)
}

fn run_git_with(
    args: Vec<String>,
    cwd: Option<&Path>,
    epoch: u64,
    resolve: impl FnOnce() -> Result<Command, String>,
) -> Result<(), String> {
    let mut command = resolve()?;
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    run_registered_git(command, epoch)
}

type LiveGitChild = std::sync::Arc<std::sync::Mutex<std::process::Child>>;

static LIVE_GIT_CHILDREN: std::sync::Mutex<Vec<LiveGitChild>> = std::sync::Mutex::new(Vec::new());

// A git child is tracked while it runs so process shutdown can stop it: a clone that
// outlives the daemon keeps writing into a data directory nobody owns any more.
fn run_registered_git(mut command: Command, epoch: u64) -> Result<(), String> {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    // git's helpers (remote-https, ssh) inherit its stderr; its own process group lets shutdown stop them all.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    // Spawn under the registry lock, so a concurrent shutdown cannot miss a child that already runs.
    let mut live = LIVE_GIT_CHILDREN
        .lock()
        .map_err(|_| git_failure("git registry lock poisoned"))?;
    if crate::git_probe::epoch() != epoch {
        return Err("plugin channel git operation cancelled".into());
    }
    let mut child = command
        .spawn()
        .map_err(|e| git_failure(&format!("failed to start git: {e}")))?;
    let mut stderr = child.stderr.take().expect("git stderr is piped");
    let child: LiveGitChild = std::sync::Arc::new(std::sync::Mutex::new(child));
    live.push(child.clone());
    drop(live);
    let mut captured = Vec::new();
    let _ = std::io::Read::read_to_end(&mut stderr, &mut captured);
    let status = child
        .lock()
        .map_err(|_| git_failure("git child lock poisoned"))?
        .wait();
    if let Ok(mut live) = LIVE_GIT_CHILDREN.lock() {
        live.retain(|tracked| !std::sync::Arc::ptr_eq(tracked, &child));
    }
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(git_failure(&String::from_utf8_lossy(&captured))),
        Err(e) => Err(git_failure(&format!("failed to wait for git: {e}"))),
    }
}

/// Stops every plugin git child still running and returns how many were stopped.
pub fn stop_live_git_children() -> usize {
    let (probes, children) = LIVE_GIT_CHILDREN
        .lock()
        .map(|mut live| (crate::git_probe::stop(), std::mem::take(&mut *live)))
        .unwrap_or_default();
    let mut stopped = 0;
    for child in children {
        let Ok(mut child) = child.lock() else {
            continue;
        };
        if kill_git_tree(&mut child) {
            let _ = child.wait();
            stopped += 1;
        }
    }
    stopped + probes
}

// Killing only git would leave its helpers holding stderr open, so the reader waits until they exit.
#[cfg(unix)]
fn kill_git_tree(child: &mut std::process::Child) -> bool {
    let Ok(group) = libc::pid_t::try_from(child.id()) else {
        return child.kill().is_ok();
    };
    // SAFETY: the group id is the leader we spawned and still hold unreaped, so it cannot name another group.
    unsafe { libc::kill(-group, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
fn kill_git_tree(child: &mut std::process::Child) -> bool {
    child.kill().is_ok()
}

pub(super) fn git_command() -> Result<Command, String> {
    let mut command = Command::new(crate::git_tool::program()?);
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(std::process::Stdio::null());
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    no_window(&mut command);
    Ok(command)
}

pub(super) fn git_failure(stderr: &str) -> String {
    log::warn!("plugin channel git operation failed: {}", stderr.trim());
    super::channel_failure::git_failure_cause(stderr)
}

#[cfg(windows)]
pub(crate) fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn no_window(_command: &mut Command) {}

#[cfg(all(test, unix))]
mod tests;

pub(super) fn system_channel_url() -> String {
    std::env::var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SYSTEM_CHANNEL_URL.to_string())
}

pub(super) fn default_plugin_channel_url() -> String {
    std::env::var("CODETERM_PLUGIN_CHANNEL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PLUGIN_CHANNEL_URL.to_string())
}

pub(super) fn app_version() -> Option<String> {
    crate::ctx_version()
}

pub(super) fn manifest_min_codeterm(dir: &Path) -> Option<String> {
    read_plugin_manifest(dir).ok().and_then(|m| m.min_codeterm)
}

pub(super) fn compat_reason(min: &str, app: Option<&str>) -> String {
    match app {
        Some(app) => format!("requires Domios \u{2265} {min}, but this is {app}"),
        None => format!("requires Domios \u{2265} {min}"),
    }
}

// no daemon); only block when we positively know app < min.
pub(super) fn compat_check(min: Option<&str>) -> Result<(), String> {
    let Some(min) = min else { return Ok(()) };
    let Some(app) = app_version() else {
        return Ok(());
    };
    let app_version =
        semver::Version::parse(&app).map_err(|_| format!("invalid Domios version: {app}"))?;
    let minimum =
        semver::Version::parse(min).map_err(|_| format!("invalid minCodeterm version: {min}"))?;
    if app_version.cmp_precedence(&minimum).is_lt() {
        Err(compat_reason(min, Some(&app)))
    } else {
        Ok(())
    }
}

pub(super) fn is_verified_install(id: &str, record: Option<&InstallRecord>) -> bool {
    record
        .map(|r| matches!(r.source, InstallSource::SystemChannel { .. }))
        .unwrap_or_else(|| is_reserved_system_id(id))
}

pub(super) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
