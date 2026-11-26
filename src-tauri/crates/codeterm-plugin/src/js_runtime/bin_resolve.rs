//! The one binary-resolution ladder the whole app shares: current process and
//! persisted Windows PATHs, then rescue dirs a GUI-launched process may miss.
//! Enumeration, spawn preflight and `host.exec` all read it.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub use codeterm_util::windows_env::EnvironmentValue;
use parking_lot::Mutex;

mod install_dirs;
mod path_source;
pub use install_dirs::{expand_install_dir, expand_install_dirs};
pub use path_source::{refresh_user_path, PathSource, SystemPathSource};

static RESOLVED_BINS: LazyLock<Mutex<HashMap<ResolutionKey, BinResolution>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResolutionKey {
    binary: String,
    roots: Vec<PathBuf>,
    names: Vec<String>,
    environment_readable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinSearchEnvironment {
    pub inherited_path: Option<OsString>,
    pub machine_path: EnvironmentValue,
    pub user_path: EnvironmentValue,
    pub pathext: Option<OsString>,
}

impl BinSearchEnvironment {
    pub fn current() -> Self {
        SystemPathSource.environment()
    }
}

#[derive(Debug, Clone)]
struct SearchContext {
    roots: Vec<PathBuf>,
    names: Vec<String>,
    path_searchable: bool,
    environment_readable: bool,
}

/// What one walk of the resolution ladder concluded. `Unknown` is the answer a
/// ladder that could not search must give: `Missing` means the search ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinResolution {
    Found(std::path::PathBuf),
    Missing,
    Unknown,
}

/// A resolution plus the directories the ladder was willing to walk, so a
/// caller can name the search in its own error instead of re-deriving it.
#[derive(Debug, Clone)]
pub struct BinProbe {
    pub resolution: BinResolution,
    pub searched: Vec<std::path::PathBuf>,
}

/// True when `p` is a regular file with an executable bit (any) set.
fn is_executable_file(p: &Path) -> bool {
    matches!(inspect_executable_file(p), FileProbe::Found(_))
}

pub(super) fn resolve_bin(bin: &str, extra_dirs: &[PathBuf]) -> PathBuf {
    match resolve_bin_memoized(bin, extra_dirs) {
        BinResolution::Found(path) => path,
        _ => PathBuf::from(bin),
    }
}

pub(super) fn resolve_bin_with_extra_dirs(bin: &str, extra_dirs: &[PathBuf]) -> PathBuf {
    resolve_bin(bin, &combined_bin_dirs(extra_dirs))
}

fn resolve_bin_memoized(bin: &str, extra_dirs: &[PathBuf]) -> BinResolution {
    let context = search_context(bin, &BinSearchEnvironment::current(), extra_dirs);
    resolve_bin_memoized_with_context(bin, &context)
}

pub fn probe_bin_with_extra_dirs(bin: &str, extra_dirs: &[PathBuf]) -> BinProbe {
    probe_bin_current(bin, &combined_bin_dirs(extra_dirs))
}

pub fn cli_binary_candidates<'a>(primary: &'a str, aliases: &'a [String]) -> Vec<&'a str> {
    let mut out = Vec::with_capacity(1 + aliases.len());
    let primary = primary.trim();
    if !primary.is_empty() {
        out.push(primary);
    }
    for alias in aliases {
        let alias = alias.trim();
        if !alias.is_empty() && !alias.contains(['/', '\\']) && !out.contains(&alias) {
            out.push(alias);
        }
    }
    out
}

pub fn probe_bin_candidates_with_extra_dirs(bins: &[&str], extra_dirs: &[PathBuf]) -> BinProbe {
    let dirs = combined_bin_dirs(extra_dirs);
    let mut saw_unknown = false;
    let mut searched = Vec::new();
    for &bin in bins {
        let probe = probe_bin_current(bin, &dirs);
        if searched.is_empty() {
            searched = probe.searched;
        }
        match probe.resolution {
            BinResolution::Found(path) => {
                return BinProbe {
                    resolution: BinResolution::Found(path),
                    searched,
                };
            }
            BinResolution::Unknown => saw_unknown = true,
            BinResolution::Missing => {}
        }
    }
    BinProbe {
        resolution: if saw_unknown {
            BinResolution::Unknown
        } else {
            BinResolution::Missing
        },
        searched,
    }
}

fn combined_bin_dirs(extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = well_known_bin_dirs();
    for dir in extra_dirs {
        if !dirs.contains(dir) {
            dirs.push(dir.clone());
        }
    }
    dirs
}

pub fn probe_bin_in(bin: &str, extra_dirs: &[PathBuf]) -> BinProbe {
    probe_bin_current(bin, extra_dirs)
}

fn probe_bin_current(bin: &str, extra_dirs: &[PathBuf]) -> BinProbe {
    probe_bin_from(bin, extra_dirs, &SystemPathSource)
}

pub fn probe_bin_from(bin: &str, extra_dirs: &[PathBuf], source: &dyn PathSource) -> BinProbe {
    probe_bin_with_environment(bin, extra_dirs, &source.environment())
}

pub fn probe_bin_with_environment(
    bin: &str,
    extra_dirs: &[PathBuf],
    environment: &BinSearchEnvironment,
) -> BinProbe {
    let context = search_context(bin, environment, extra_dirs);
    BinProbe {
        resolution: resolve_bin_memoized_with_context(bin, &context),
        searched: context.roots,
    }
}

fn resolve_bin_memoized_with_context(bin: &str, context: &SearchContext) -> BinResolution {
    let key = resolution_key(bin, context);
    let cached = { RESOLVED_BINS.lock().get(&key).cloned() };
    if let Some(hit) = cached {
        if let BinResolution::Found(path) = &hit {
            if cached_path_is_current(bin, path, context) {
                return hit;
            }
        }
        RESOLVED_BINS.lock().remove(&key);
    }
    let resolved = resolve_bin_with_context(bin, context);
    if matches!(resolved, BinResolution::Found(_)) {
        RESOLVED_BINS.lock().insert(key, resolved.clone());
    }
    resolved
}

fn cached_path_is_current(bin: &str, path: &Path, context: &SearchContext) -> bool {
    if !is_executable_file(path) {
        return false;
    }
    if has_path_separator(bin) {
        return true;
    }

    for root in &context.roots {
        for name in &context.names {
            let candidate = root.join(name);
            if candidate == path {
                return true;
            }
            if matches!(inspect_executable_file(&candidate), FileProbe::Found(_)) {
                return false;
            }
        }
    }
    false
}

fn resolution_key(bin: &str, context: &SearchContext) -> ResolutionKey {
    ResolutionKey {
        binary: bin.to_string(),
        roots: context.roots.clone(),
        names: context.names.clone(),
        environment_readable: context.environment_readable,
    }
}

fn search_context(
    bin: &str,
    environment: &BinSearchEnvironment,
    extra_dirs: &[PathBuf],
) -> SearchContext {
    let mut roots = Vec::new();
    let mut path_searchable = false;
    let mut environment_readable = true;
    if let Some(path) = environment
        .inherited_path
        .as_ref()
        .filter(|p| !p.is_empty())
    {
        roots.extend(split_path_entries(path));
        path_searchable = true;
    }
    for value in [&environment.machine_path, &environment.user_path] {
        match value {
            EnvironmentValue::Value(path) if !path.is_empty() => {
                roots.extend(split_path_entries(path));
                path_searchable = true;
            }
            EnvironmentValue::Value(_) | EnvironmentValue::Missing => {}
            EnvironmentValue::Unreadable(_) => environment_readable = false,
        }
    }
    for dir in extra_dirs {
        if !roots.contains(dir) {
            roots.push(dir.clone());
        }
    }
    SearchContext {
        roots,
        names: probe_names(bin, environment.pathext.as_deref()),
        path_searchable,
        environment_readable,
    }
}

/// Joins the same search ladder used by `probe_bin_with_extra_dirs` into a deduplicated `PATH`
/// value so spawned subprocesses find interpreters and helper tools.
pub fn merged_search_path(
    environment: &BinSearchEnvironment,
    extra_dirs: &[PathBuf],
) -> Option<OsString> {
    let mut roots = Vec::new();
    let mut push_unique = |dir: PathBuf| {
        if !dir.as_os_str().is_empty() && !roots.contains(&dir) {
            roots.push(dir);
        }
    };
    if let Some(path) = environment
        .inherited_path
        .as_ref()
        .filter(|p| !p.is_empty())
    {
        for dir in split_path_entries(path) {
            push_unique(dir);
        }
    }
    for value in [&environment.machine_path, &environment.user_path] {
        if let EnvironmentValue::Value(path) = value {
            if !path.is_empty() {
                for dir in split_path_entries(path) {
                    push_unique(dir);
                }
            }
        }
    }
    for dir in extra_dirs {
        push_unique(dir.clone());
    }
    (!roots.is_empty())
        .then(|| join_valid_paths(&roots))
        .flatten()
}

pub fn merged_search_path_with_extra_dirs(extra_dirs: &[PathBuf]) -> Option<OsString> {
    merged_search_path(
        &BinSearchEnvironment::current(),
        &combined_bin_dirs(extra_dirs),
    )
}

fn join_valid_paths(roots: &[PathBuf]) -> Option<OsString> {
    if let Ok(joined) = std::env::join_paths(roots) {
        return Some(joined);
    }
    let valid: Vec<_> = roots
        .iter()
        .filter(|dir| std::env::join_paths(std::iter::once(dir)).is_ok())
        .collect();
    (!valid.is_empty())
        .then(|| std::env::join_paths(valid).ok())
        .flatten()
}

fn split_path_entries(value: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(value)
        .map(unquote_path_entry)
        .collect()
}

#[cfg(windows)]
fn unquote_path_entry(path: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.first() == Some(&(b'"' as u16)) && value.last() == Some(&(b'"' as u16)) {
        value.remove(0);
        value.pop();
    }
    PathBuf::from(OsString::from_wide(&value))
}

#[cfg(not(windows))]
fn unquote_path_entry(path: PathBuf) -> PathBuf {
    path
}

fn resolve_bin_with_context(bin: &str, context: &SearchContext) -> BinResolution {
    if has_path_separator(bin) {
        return match inspect_executable_file(Path::new(bin)) {
            FileProbe::Found(path) => BinResolution::Found(path),
            FileProbe::Missing => BinResolution::Missing,
            FileProbe::Unreadable => BinResolution::Unknown,
        };
    }
    match probe_dirs(&context.roots, &context.names) {
        FileProbe::Found(path) => BinResolution::Found(path),
        FileProbe::Unreadable => BinResolution::Unknown,
        FileProbe::Missing if context.path_searchable && context.environment_readable => {
            BinResolution::Missing
        }
        FileProbe::Missing => BinResolution::Unknown,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileProbe {
    Found(PathBuf),
    Missing,
    Unreadable,
}

fn inspect_executable_file(path: &Path) -> FileProbe {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return FileProbe::Missing,
        Err(_) => return FileProbe::Unreadable,
    };
    if !metadata.is_file() {
        return FileProbe::Missing;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return FileProbe::Missing;
        }
    }
    FileProbe::Found(path.to_path_buf())
}

fn probe_dirs(roots: &[PathBuf], names: &[String]) -> FileProbe {
    let mut unreadable = false;
    for dir in roots {
        for name in names {
            match inspect_executable_file(&dir.join(name)) {
                FileProbe::Found(path) => return FileProbe::Found(path),
                FileProbe::Missing => {}
                FileProbe::Unreadable => unreadable = true,
            }
        }
    }
    if unreadable {
        FileProbe::Unreadable
    } else {
        FileProbe::Missing
    }
}

fn has_path_separator(bin: &str) -> bool {
    bin.contains('/') || bin.contains('\\')
}

fn probe_names(bin: &str, pathext: Option<&OsStr>) -> Vec<String> {
    #[cfg(windows)]
    {
        if Path::new(bin).extension().is_some() {
            return vec![bin.to_string()];
        }
        let pathext = pathext
            .and_then(|value| value.to_str())
            .unwrap_or(".COM;.EXE;.BAT;.CMD");
        pathext
            .split(';')
            .map(str::trim)
            .filter(|extension| extension.starts_with('.'))
            .map(|extension| format!("{bin}{}", extension.to_ascii_lowercase()))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = pathext;
        vec![bin.to_string()]
    }
}

pub fn forget_resolved_bins() {
    RESOLVED_BINS.lock().clear();
}

pub fn forget_resolved_bin(bin: &str) {
    RESOLVED_BINS.lock().retain(|key, _| key.binary != bin);
}

pub fn resolved_bin_path(bin: &str) -> Option<PathBuf> {
    let context = search_context(
        bin,
        &BinSearchEnvironment::current(),
        &well_known_bin_dirs(),
    );
    resolved_bin_path_in_context(bin, &context)
}

fn resolved_bin_path_in_context(bin: &str, context: &SearchContext) -> Option<PathBuf> {
    let BinResolution::Found(hit) = RESOLVED_BINS
        .lock()
        .get(&resolution_key(bin, context))
        .cloned()?
    else {
        return None;
    };
    is_executable_file(&hit).then_some(hit)
}

/// Well-known dirs a GUI-launched app may miss from a login shell's PATH.
/// Home comes from `dirs`, not `$HOME`: Windows leaves `HOME` unset, which used
/// to drop every home-relative candidate on the platform that needs them most.
pub fn well_known_bin_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        std::path::PathBuf::from("/opt/homebrew/bin"),
        std::path::PathBuf::from("/usr/local/bin"),
        std::path::PathBuf::from("/usr/bin"),
        std::path::PathBuf::from("/snap/bin"),
    ];
    let home = home_dir();
    if let Some(home) = home.as_deref() {
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
        dirs.push(home.join(".volta/bin"));
        dirs.extend(crate::provider_command::versioned_bin_dirs(
            &home.join(".nvm/versions/node"),
        ));
        dirs.push(home.join(".bun/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join("scoop/shims"));
        dirs.push(home.join("AppData/Roaming/npm"));
        dirs.push(home.join("AppData/Local/Programs"));
    }
    for dir in install_dirs::npm_global_bin_dirs(
        &|key| std::env::var_os(key),
        home.as_deref(),
        cfg!(windows),
    ) {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

pub(crate) fn current_home_dir() -> Option<std::path::PathBuf> {
    home_dir()
}

fn home_dir() -> Option<std::path::PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        return Some(std::path::PathBuf::from(home));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()) {
        return Some(std::path::PathBuf::from(profile));
    }
    dirs::home_dir()
}

#[cfg(test)]
mod tests;
