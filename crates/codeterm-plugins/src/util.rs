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
    let tmp = cache_path.with_extension("git-clone");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| format!("clear {}: {e}", tmp.display()))?;
    }
    run_git([
        "clone",
        "--depth",
        "1",
        &source.base,
        &tmp.to_string_lossy(),
    ])?;
    if let Some(git_ref) = &source.git_ref {
        run_git_in(&tmp, ["fetch", "--depth", "1", "origin", git_ref])?;
        run_git_in(&tmp, ["checkout", "FETCH_HEAD"])?;
    }
    if let Some(subdir) = &source.subdir {
        validate_relative_path(subdir)?;
        copy_dir_filtered(&tmp.join(subdir), cache_path)?;
        fs::remove_dir_all(&tmp).map_err(|e| format!("remove {}: {e}", tmp.display()))?;
    } else {
        fs::rename(&tmp, cache_path).map_err(|e| format!("rename channel cache: {e}"))?;
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

pub(super) fn run_git<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

pub(super) fn run_git_in<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<(), String> {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

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

// Fail-open when min is absent or the running version is unknown (raw CLI with
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

// Best-effort version for a system plugin not on disk: read the system channel
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
