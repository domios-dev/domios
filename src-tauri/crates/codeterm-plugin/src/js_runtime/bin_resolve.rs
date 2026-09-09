//! The one binary-resolution ladder the whole app shares: current process and
//! persisted Windows PATHs, then rescue dirs a GUI-launched process may miss.
//! Enumeration, spawn preflight and `host.exec` all read it.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub use codeterm_util::windows_env::EnvironmentValue;
use codeterm_util::windows_env::read_persisted_path;
use parking_lot::Mutex;

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
        let persisted = read_persisted_path();
        Self {
            inherited_path: std::env::var_os("PATH"),
            machine_path: persisted.machine,
            user_path: persisted.user,
            pathext: std::env::var_os("PATHEXT"),
        }
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

fn resolve_bin_memoized(bin: &str, extra_dirs: &[PathBuf]) -> BinResolution {
    let context = search_context(bin, &BinSearchEnvironment::current(), extra_dirs);
    resolve_bin_memoized_with_context(bin, &context)
}

pub fn probe_bin(bin: &str) -> BinProbe {
    probe_bin_current(bin, &well_known_bin_dirs())
}

pub fn probe_bin_in(bin: &str, extra_dirs: &[PathBuf]) -> BinProbe {
    probe_bin_current(bin, extra_dirs)
}

fn probe_bin_current(bin: &str, extra_dirs: &[PathBuf]) -> BinProbe {
    let environment = BinSearchEnvironment::current();
    let context = search_context(bin, &environment, extra_dirs);
    BinProbe {
        resolution: resolve_bin_memoized_with_context(bin, &context),
        searched: context.roots,
    }
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
    if let Some(path) = environment.inherited_path.as_ref().filter(|p| !p.is_empty()) {
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
        let mut names = vec![bin.to_string()];
        if Path::new(bin).extension().is_some() {
            return names;
        }
        let pathext = pathext
            .and_then(|value| value.to_str())
            .unwrap_or(".COM;.EXE;.BAT;.CMD");
        names.extend(
            pathext
                .split(';')
                .map(str::trim)
                .filter(|extension| extension.starts_with('.'))
                .map(|extension| format!("{bin}{}", extension.to_ascii_lowercase())),
        );
        names
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
    let context = search_context(bin, &BinSearchEnvironment::current(), &well_known_bin_dirs());
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
    if let Some(home) = home_dir() {
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
        dirs.push(home.join(".volta/bin"));
        dirs.push(home.join(".bun/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join("scoop/shims"));
        dirs.push(home.join("AppData/Roaming/npm"));
        dirs.push(home.join("AppData/Local/Programs"));
    }
    dirs
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
mod tests {
    use super::*;
    use crate::js_runtime::PROCESS_TEST_LOCK as TEST_LOCK;

    fn environment_with_paths(
        inherited: Option<&Path>,
        machine: EnvironmentValue,
        user: EnvironmentValue,
    ) -> BinSearchEnvironment {
        let inherited_path = inherited
            .map(|path| std::env::join_paths([path.as_os_str()]).unwrap());
        BinSearchEnvironment {
            inherited_path,
            machine_path: machine,
            user_path: user,
            pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
        }
    }

    fn cached_count(bin: &str) -> usize {
        RESOLVED_BINS
            .lock()
            .keys()
            .filter(|key| key.binary == bin)
            .count()
    }

    #[test]
    fn resolve_bin_probes_fallback_dir_when_not_on_path() {
        // A bin absent from PATH must still resolve through a fallback candidate
        // dir, which is the only rescue a GUI-launched daemon has.
        let name = format!("bwprobe_{}", uuid::Uuid::new_v4().simple());
        let dir = std::env::temp_dir().join(format!(
            "host_exec_resolve_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            resolved, bin_path,
            "resolve_bin must find a bin via the provided fallback dir, not just PATH"
        );
    }

    #[test]
    fn resolve_bin_preserves_process_path_precedence() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        #[cfg(windows)]
        let name = format!("host_exec_{}.cmd", uuid::Uuid::new_v4().simple());
        #[cfg(not(windows))]
        let name = format!("host_exec_{}", uuid::Uuid::new_v4().simple());

        let path_dir =
            std::env::temp_dir().join(format!("host_exec_path_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path_dir).unwrap();
        let expected = path_dir.join(&name);
        std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&expected).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&expected, perm).unwrap();
        }

        let dir =
            std::env::temp_dir().join(format!("host_exec_decoy_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let decoy = dir.join(&name);
        std::fs::write(&decoy, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&decoy).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&decoy, perm).unwrap();
        }

        let environment = environment_with_paths(
            Some(&path_dir),
            EnvironmentValue::Missing,
            EnvironmentValue::Value(std::env::join_paths([dir.as_os_str()]).unwrap()),
        );
        let resolved = probe_bin_with_environment(&name, &[], &environment).resolution;
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&path_dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved,
            BinResolution::Found(expected),
            "the inherited process PATH must retain precedence over persisted PATH"
        );
    }

    /// The GUI-launched daemon that shipped `available:false` had no `HOME` at
    /// all, so every home-relative candidate vanished and the ladder had
    /// nothing left to try. Home must survive an unset `HOME`.
    #[test]
    fn well_known_bin_dirs_reach_user_local_bin_without_home() {
        let _guard = TEST_LOCK.lock();
        let mut process_env = codeterm_util::test_env::EnvGuard::lock();
        let home =
            std::env::temp_dir().join(format!("host_exec_home_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(home.join(".local/bin")).unwrap();
        process_env.remove("HOME");
        process_env.set("USERPROFILE", &home);

        let dirs = well_known_bin_dirs();
        let _ = std::fs::remove_dir_all(&home);
        assert!(
            dirs.contains(&home.join(".local/bin")),
            "an unset HOME must not drop the home-relative candidates: {dirs:?}"
        );
    }

    /// The installed file was `claude.exe`; the probe only ever looked for
    /// `claude`, so the rescue dir could not answer even when it held the CLI.
    #[test]
    fn resolve_bin_probe_accepts_platform_executable_suffix() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwext_{}", uuid::Uuid::new_v4().simple());
        let dir =
            std::env::temp_dir().join(format!("host_exec_ext_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let bin_path = dir.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let environment = environment_with_paths(
            None,
            EnvironmentValue::Missing,
            EnvironmentValue::Value(std::env::join_paths([dir.as_os_str()]).unwrap()),
        );
        let resolved = probe_bin_with_environment(&name, &[], &environment).resolution;
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved,
            BinResolution::Found(bin_path),
            "a probe dir holding the platform-suffixed executable must answer the bare bin"
        );
    }

    #[test]
    fn explicit_and_relative_paths_are_checked_directly() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        #[cfg(windows)]
        let name = format!("bwdirect_{}.exe", uuid::Uuid::new_v4().simple());
        #[cfg(not(windows))]
        let name = format!("bwdirect_{}", uuid::Uuid::new_v4().simple());
        let root = std::env::current_dir()
            .unwrap()
            .join(format!("bin_resolve_paths_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let absolute = root.join(&name);
        std::fs::write(&absolute, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&absolute).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&absolute, perm).unwrap();
        }
        let environment = environment_with_paths(
            None,
            EnvironmentValue::Missing,
            EnvironmentValue::Missing,
        );
        let explicit = probe_bin_with_environment(
            absolute.to_str().unwrap(),
            &[],
            &environment,
        )
        .resolution;
        let relative_input = absolute
            .strip_prefix(std::env::current_dir().unwrap())
            .unwrap()
            .to_path_buf();
        let relative = probe_bin_with_environment(
            relative_input.to_str().unwrap(),
            &[],
            &environment,
        )
            .resolution;
        let _ = std::fs::remove_dir_all(&root);
        RESOLVED_BINS.lock().clear();

        assert_eq!(explicit, BinResolution::Found(absolute));
        assert_eq!(relative, BinResolution::Found(relative_input));
    }

    #[test]
    fn non_runnable_candidates_are_not_reported_as_found() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwnonrun_{}", uuid::Uuid::new_v4().simple());
        let root = std::env::temp_dir().join(format!(
            "bin_resolve_nonrun_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(root.join(&name)).unwrap();
        let environment = environment_with_paths(
            Some(&root),
            EnvironmentValue::Missing,
            EnvironmentValue::Missing,
        );
        let probe = probe_bin_with_environment(&name, &[], &environment);
        #[cfg(unix)]
        let non_executable = {
            let file_name = format!("{name}_file");
            let file = root.join(&file_name);
            std::fs::write(&file, b"not executable").unwrap();
            let mut perm = std::fs::metadata(&file).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            perm.set_mode(0o644);
            std::fs::set_permissions(&file, perm).unwrap();
            probe_bin_with_environment(&file_name, &[], &environment).resolution
        };
        let _ = std::fs::remove_dir_all(&root);
        RESOLVED_BINS.lock().clear();

        assert_eq!(probe.resolution, BinResolution::Missing);
        #[cfg(unix)]
        assert_eq!(non_executable, BinResolution::Missing);
    }

    #[test]
    fn an_earlier_path_candidate_takes_precedence_after_it_appears() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwprecedence_{}", uuid::Uuid::new_v4().simple());
        let earlier = std::env::temp_dir().join(format!(
            "bin_resolve_earlier_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let later = std::env::temp_dir().join(format!(
            "bin_resolve_later_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&earlier).unwrap();
        std::fs::create_dir_all(&later).unwrap();
        #[cfg(windows)]
        let later_path = later.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let later_path = later.join(&name);
        std::fs::write(&later_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&later_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&later_path, perm).unwrap();
        }
        let environment = environment_with_paths(
            None,
            EnvironmentValue::Value(
                std::env::join_paths([earlier.as_os_str(), later.as_os_str()]).unwrap(),
            ),
            EnvironmentValue::Missing,
        );
        let first = probe_bin_with_environment(&name, &[], &environment).resolution;

        #[cfg(windows)]
        let earlier_path = earlier.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let earlier_path = earlier.join(&name);
        std::fs::write(&earlier_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&earlier_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&earlier_path, perm).unwrap();
        }
        let second = probe_bin_with_environment(&name, &[], &environment).resolution;
        let _ = std::fs::remove_dir_all(&earlier);
        let _ = std::fs::remove_dir_all(&later);
        RESOLVED_BINS.lock().clear();

        assert_eq!(first, BinResolution::Found(later_path));
        assert_eq!(second, BinResolution::Found(earlier_path));
    }

    #[test]
    fn a_miss_is_reprobed_after_the_binary_appears_without_forget() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwmemo_{}", uuid::Uuid::new_v4().simple());
        let dir =
            std::env::temp_dir().join(format!("host_exec_memo_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();

        let missed = resolve_bin(&name, std::slice::from_ref(&dir));
        assert_eq!(missed, std::path::PathBuf::from(&name));
        assert_eq!(resolved_bin_path(&name), None);

        #[cfg(windows)]
        let bin_path = dir.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let reprobed = resolve_bin(&name, std::slice::from_ref(&dir));
        let watched = resolved_bin_path_in_context(
            &name,
            &search_context(&name, &BinSearchEnvironment::current(), std::slice::from_ref(&dir)),
        );
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(reprobed, bin_path);
        assert_eq!(watched, Some(bin_path));
    }

    #[test]
    fn removing_a_found_file_invalidates_the_cached_available_result() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwremoved_{}", uuid::Uuid::new_v4().simple());
        let dir = std::env::temp_dir().join(format!(
            "bin_resolve_removed_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(windows)]
        let bin_path = dir.join(format!("{name}.exe"));
        #[cfg(not(windows))]
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let environment = environment_with_paths(
            Some(&dir),
            EnvironmentValue::Missing,
            EnvironmentValue::Missing,
        );
        assert_eq!(
            probe_bin_with_environment(&name, &[], &environment).resolution,
            BinResolution::Found(bin_path.clone())
        );
        std::fs::remove_file(&bin_path).unwrap();
        assert_eq!(
            probe_bin_with_environment(&name, &[], &environment).resolution,
            BinResolution::Missing
        );
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
    }

    #[test]
    fn persisted_path_finds_a_binary_missing_from_the_daemon_path() {
        let _guard = TEST_LOCK.lock();
        let name = format!("bwpersisted_{}", uuid::Uuid::new_v4().simple());
        let stale = std::env::temp_dir().join(format!(
            "bin_resolve_stale_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let installed = std::env::temp_dir().join(format!(
            "bin_resolve_installed spaced_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::create_dir_all(&installed).unwrap();
        let expected = installed.join(&name);

        let missing_environment = environment_with_paths(
            Some(&stale),
            EnvironmentValue::Missing,
            EnvironmentValue::Missing,
        );
        assert_eq!(
            probe_bin_with_environment(&name, &[], &missing_environment).resolution,
            BinResolution::Missing
        );
        std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&expected).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&expected, perm).unwrap();
        }

        let environment = environment_with_paths(
            Some(&stale),
            EnvironmentValue::Missing,
            EnvironmentValue::Value(std::env::join_paths([installed.as_os_str()]).unwrap()),
        );
        let probe = probe_bin_with_environment(&name, &[], &environment);
        assert_eq!(probe.resolution, BinResolution::Found(expected.clone()));
        assert_eq!(probe.searched, vec![stale.clone(), installed.clone()]);
        let _ = std::fs::remove_dir_all(expected.parent().unwrap());
        let _ = std::fs::remove_dir_all(&stale);
    }

    #[test]
    fn unreadable_persisted_path_is_unknown_until_a_search_can_answer() {
        let _guard = TEST_LOCK.lock();
        let name = format!("bwunreadable_{}", uuid::Uuid::new_v4().simple());
        let stale = std::env::temp_dir().join(format!(
            "bin_resolve_unreadable_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let installed = std::env::temp_dir().join(format!(
            "bin_resolve_recovered_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::create_dir_all(&installed).unwrap();
        let expected = installed.join(&name);
        std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&expected).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&expected, perm).unwrap();
        }

        let unreadable = environment_with_paths(
            Some(&stale),
            EnvironmentValue::Missing,
            EnvironmentValue::Unreadable("fixture read failure".into()),
        );
        assert_eq!(
            probe_bin_with_environment(&name, &[], &unreadable).resolution,
            BinResolution::Unknown
        );
        let readable = environment_with_paths(
            Some(&stale),
            EnvironmentValue::Missing,
            EnvironmentValue::Value(std::env::join_paths([installed.as_os_str()]).unwrap()),
        );
        assert_eq!(
            probe_bin_with_environment(&name, &[], &readable).resolution,
            BinResolution::Found(expected.clone())
        );
        let _ = std::fs::remove_dir_all(expected.parent().unwrap());
        let _ = std::fs::remove_dir_all(&stale);
    }

    /// A ladder with no PATH to read never searched, so its miss is an
    /// unanswered question. Collapsing it into "missing" is the defect.
    #[test]
    fn probe_reports_unknown_when_the_path_could_not_be_searched() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwunk_{}", uuid::Uuid::new_v4().simple());
        let empty = std::env::temp_dir().join(format!(
            "bin_resolve_unknown_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&empty).unwrap();

        let searchable = environment_with_paths(
            Some(&empty),
            EnvironmentValue::Missing,
            EnvironmentValue::Missing,
        );
        let searched = probe_bin_with_environment(&name, &[], &searchable);
        RESOLVED_BINS.lock().clear();
        let unsearchable = probe_bin_with_environment(
            &name,
            std::slice::from_ref(&empty),
            &BinSearchEnvironment {
                inherited_path: None,
                machine_path: EnvironmentValue::Missing,
                user_path: EnvironmentValue::Missing,
                pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
            },
        );
        let _ = std::fs::remove_dir_all(&empty);
        RESOLVED_BINS.lock().clear();

        assert_eq!(searched.resolution, BinResolution::Missing);
        assert_eq!(
            unsearchable.resolution,
            BinResolution::Unknown,
            "a search that could not run must not report a definitive miss"
        );
    }

    /// An unknown that got memoized would freeze a machine into a wrong
    /// capability answer for the whole process lifetime.
    #[test]
    fn unknown_is_never_memoized() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwnomemo_{}", uuid::Uuid::new_v4().simple());
        let environment = BinSearchEnvironment {
            inherited_path: None,
            machine_path: EnvironmentValue::Missing,
            user_path: EnvironmentValue::Missing,
            pathext: Some(OsString::from(".COM;.EXE;.BAT;.CMD")),
        };
        assert_eq!(
            probe_bin_with_environment(&name, &[], &environment).resolution,
            BinResolution::Unknown
        );
        let memoized = cached_count(&name) != 0;
        RESOLVED_BINS.lock().clear();
        assert!(!memoized, "an unanswered probe must stay retryable");
    }

    #[test]
    fn probe_reports_the_directories_it_was_willing_to_walk() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwsearch_{}", uuid::Uuid::new_v4().simple());
        let dir = std::env::temp_dir().join(format!(
            "bin_resolve_search_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let probe = probe_bin_in(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert!(
            probe.searched.contains(&dir),
            "the rescue dir must be nameable in a refusal: {:?}",
            probe.searched
        );
    }

    #[test]
    fn resolve_bin_via_probe_preserves_basename() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwbase_{}", uuid::Uuid::new_v4().simple());
        let dir =
            std::env::temp_dir().join(format!("host_exec_base_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved.file_name().and_then(|n| n.to_str()),
            Some(name.as_str()),
            "a probe-dir resolution must preserve the requested basename"
        );
    }
}
