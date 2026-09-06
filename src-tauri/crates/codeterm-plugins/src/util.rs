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
    let tmp_string = tmp.to_string_lossy().to_string();
    let mut clone_args = vec![
        "clone".to_string(),
        "-c".to_string(),
        "core.longpaths=true".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "--filter=blob:none".to_string(),
    ];
    if source.subdir.is_some() {
        clone_args.push("--sparse".to_string());
    }
    clone_args.push(source.base.clone());
    clone_args.push(tmp_string);
    run_git(clone_args)?;
    if let Some(git_ref) = &source.git_ref {
        run_git_in(
            tmp,
            vec![
                "fetch".to_string(),
                "--depth".to_string(),
                "1".to_string(),
                "origin".to_string(),
                git_ref.clone(),
            ],
        )?;
        run_git_in(tmp, vec!["checkout".to_string(), "FETCH_HEAD".to_string()])?;
    }
    if let Some(subdir) = &source.subdir {
        validate_relative_path(subdir)?;
        run_git_in(
            tmp,
            vec![
                "sparse-checkout".to_string(),
                "set".to_string(),
                "--cone".to_string(),
                subdir.clone(),
            ],
        )?;
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

pub(super) fn run_git(args: Vec<String>) -> Result<(), String> {
    let mut command = git_command();
    let out = command
        .args(&args)
        .output()
        .map_err(|e| format!("run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

pub(super) fn run_git_in(cwd: &Path, args: Vec<String>) -> Result<(), String> {
    let mut command = git_command();
    let out = command
        .current_dir(cwd)
        .args(&args)
        .output()
        .map_err(|e| format!("run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn git_command() -> Command {
    #[cfg(test)]
    let program = std::env::var("CODETERM_TEST_GIT_BIN").unwrap_or_else(|_| "git".to_string());
    #[cfg(not(test))]
    let program = "git";

    let mut command = Command::new(program);
    no_window(&mut command);
    command
}

#[cfg(windows)]
fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_command: &mut Command) {}

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
        Some(app) => format!("requires CodeTerm \u{2265} {min}, but this is {app}"),
        None => format!("requires CodeTerm \u{2265} {min}"),
    }
}

// no daemon); only block when we positively know app < min.
pub(super) fn compat_check(min: Option<&str>) -> Result<(), String> {
    let Some(min) = min else { return Ok(()) };
    let Some(app) = app_version() else {
        return Ok(());
    };
    if version_cmp(&app, min).is_lt() {
        Err(compat_reason(min, Some(&app)))
    } else {
        Ok(())
    }
}

// cache manifest if it has already been fetched. Never triggers a network call.
pub(super) fn system_channel_version(id: &str) -> Option<String> {
    let reg = read_channel_registry().ok()?;
    let ch = reg.channels.get(SYSTEM_CHANNEL_NAME)?;
    let manifest = read_channel_manifest(Path::new(&ch.path)).ok()?;
    manifest
        .plugins
        .into_iter()
        .find(|p| p.id == id)
        .and_then(|p| p.version)
}

pub(super) fn is_verified_install(id: &str, record: Option<&InstallRecord>) -> bool {
    record
        .map(|r| matches!(r.source, InstallSource::SystemChannel { .. }))
        .unwrap_or_else(|| is_reserved_system_id(id))
}

pub(super) fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
