//! The one binary-resolution ladder the whole app shares: `which` over the
//! daemon PATH, then a probe of well-known rescue dirs a GUI-launched process
//! may not inherit. Enumeration, spawn preflight and `host.exec` all read it,
//! so a second ladder anywhere is a bug, not an optimisation.

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;

static RESOLVED_BINS: LazyLock<Mutex<HashMap<String, BinResolution>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
fn is_executable_file(p: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode() & 0o111 != 0;
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve `bin` to a runnable path, memoized per process. Ladder: (1) `which`
/// over the daemon PATH — unchanged when it hits; (2) on a miss, probe each
/// `extra_dirs` entry in order for the first executable regular file named
/// `bin`, or `bin` plus one of the platform's executable suffixes; (3) fall
/// back to the bare name (OS resolves at spawn). The fallback only WIDENS
/// discovery: a probe may add an executable suffix, never change the
/// allowlisted basename.
pub(super) fn resolve_bin(bin: &str, extra_dirs: &[std::path::PathBuf]) -> std::path::PathBuf {
    match resolve_bin_memoized(bin, extra_dirs) {
        BinResolution::Found(path) => path,
        _ => std::path::PathBuf::from(bin),
    }
}

/// One memoized walk of the ladder. `Unknown` is never memoized: a search that
/// could not run must be retried, not frozen into a capability answer.
fn resolve_bin_memoized(bin: &str, extra_dirs: &[std::path::PathBuf]) -> BinResolution {
    if let Some(hit) = RESOLVED_BINS.lock().get(bin) {
        return hit.clone();
    }
    let resolved = resolve_bin_uncached(bin, extra_dirs);
    if resolved != BinResolution::Unknown {
        RESOLVED_BINS
            .lock()
            .insert(bin.to_string(), resolved.clone());
    }
    resolved
}

/// The shared resolver as a capability question: what does THIS machine say
/// about `bin`, and where was it looked for. Enumeration and spawn preflight
/// both read this rather than growing a second ladder.
pub fn probe_bin(bin: &str) -> BinProbe {
    probe_bin_in(bin, &well_known_bin_dirs())
}

pub fn probe_bin_in(bin: &str, extra_dirs: &[std::path::PathBuf]) -> BinProbe {
    BinProbe {
        resolution: resolve_bin_memoized(bin, extra_dirs),
        searched: search_roots(extra_dirs),
    }
}

/// PATH entries plus the rescue dirs, in ladder order — the search an error
/// message has to be able to name.
fn search_roots(extra_dirs: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    for dir in extra_dirs {
        if !roots.contains(dir) {
            roots.push(dir.clone());
        }
    }
    roots
}

/// Drop every memoized resolution so the next call re-walks the ladder. A
/// self-updated or newly installed CLI is otherwise invisible for the process
/// lifetime, which turns one failed probe into a permanent capability answer.
pub fn forget_resolved_bins() {
    RESOLVED_BINS.lock().clear();
}

/// Drop one memoized resolution. An explicit user re-check must re-walk the
/// ladder for that binary without discarding every other provider's answer.
pub fn forget_resolved_bin(bin: &str) {
    RESOLVED_BINS.lock().remove(bin);
}

/// The concrete file a prior resolution landed on, when it landed on one at
/// all. A bare-name fallback is not a path, so it yields `None` rather than a
/// path nothing can watch.
pub fn resolved_bin_path(bin: &str) -> Option<std::path::PathBuf> {
    let BinResolution::Found(hit) = RESOLVED_BINS.lock().get(bin).cloned()? else {
        return None;
    };
    (hit.parent().is_some_and(|p| !p.as_os_str().is_empty()) && hit.is_file()).then_some(hit)
}

/// File names that answer a probe for `bin`: the bare name, plus the
/// platform's executable suffixes so a Windows `claude.exe` answers `claude`.
#[cfg(windows)]
fn probe_names(bin: &str) -> Vec<String> {
    let mut names = vec![bin.to_string()];
    if std::path::Path::new(bin).extension().is_none() {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        for ext in pathext
            .split(';')
            .map(str::trim)
            .filter(|e| e.starts_with('.'))
        {
            names.push(format!("{bin}{}", ext.to_ascii_lowercase()));
        }
    }
    names
}

#[cfg(not(windows))]
fn probe_names(bin: &str) -> Vec<String> {
    vec![bin.to_string()]
}

fn resolve_bin_uncached(bin: &str, extra_dirs: &[std::path::PathBuf]) -> BinResolution {
    let path_was_searchable = match which::which(bin) {
        Ok(p) => return BinResolution::Found(p),
        Err(which::Error::CannotFindBinaryPath) => path_env_readable(),
        Err(_) => false,
    };
    if let Some(found) = probe_extra_dirs(bin, extra_dirs) {
        return BinResolution::Found(found);
    }
    if path_was_searchable {
        BinResolution::Missing
    } else {
        BinResolution::Unknown
    }
}

/// A daemon that inherited no PATH never searched it, so a miss there is an
/// unanswered question rather than an absent binary.
fn path_env_readable() -> bool {
    std::env::var_os("PATH").is_some_and(|path| !path.is_empty())
}

fn probe_extra_dirs(bin: &str, extra_dirs: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    let names = probe_names(bin);
    for dir in extra_dirs {
        for name in &names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                debug_assert!(
                    candidate
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|found| found == bin
                            || found
                                .strip_prefix(bin)
                                .is_some_and(|suffix| suffix.starts_with('.'))),
                    "a probe may add an executable suffix, never change the allowlisted bin"
                );
                return Some(candidate);
            }
        }
    }
    None
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
    fn resolve_bin_ignores_probe_dirs_when_which_succeeds() {
        let _guard = TEST_LOCK.lock();
        let mut process_env = codeterm_util::test_env::EnvGuard::lock();
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

        let old_path = std::env::var_os("PATH");
        let mut paths = vec![path_dir.clone()];
        if let Some(old) = old_path.as_deref() {
            paths.extend(std::env::split_paths(old));
        }
        process_env.set("PATH", std::env::join_paths(paths).unwrap());

        // Decoy dir holding a same-named executable must be ignored: which() wins.
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

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&path_dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved, expected,
            "which() result must win over probe dirs"
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

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved, bin_path,
            "a probe dir holding the platform-suffixed executable must answer the bare bin"
        );
    }

    /// A memoized miss outlives the install that fixes it, so a refresh that
    /// does not clear the memo rewrites the same wrong answer forever.
    #[test]
    fn forget_resolved_bins_reprobes_after_the_binary_appears() {
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

        assert_eq!(
            resolve_bin(&name, std::slice::from_ref(&dir)),
            std::path::PathBuf::from(&name),
            "the memo must still be serving the stale miss before it is cleared"
        );
        forget_resolved_bins();
        let reprobed = resolve_bin(&name, std::slice::from_ref(&dir));
        let watched = resolved_bin_path(&name);
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(reprobed, bin_path);
        assert_eq!(watched, Some(bin_path));
    }

    /// A ladder with no PATH to read never searched, so its miss is an
    /// unanswered question. Collapsing it into "missing" is the defect.
    #[test]
    fn probe_reports_unknown_when_the_path_could_not_be_searched() {
        let _guard = TEST_LOCK.lock();
        let mut process_env = codeterm_util::test_env::EnvGuard::lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwunk_{}", uuid::Uuid::new_v4().simple());
        let empty = std::env::temp_dir().join(format!(
            "bin_resolve_unknown_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&empty).unwrap();

        let searched = probe_bin_in(&name, std::slice::from_ref(&empty));
        process_env.remove("PATH");
        let unsearchable = probe_bin_in(&name, std::slice::from_ref(&empty));
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
        let mut process_env = codeterm_util::test_env::EnvGuard::lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwnomemo_{}", uuid::Uuid::new_v4().simple());
        process_env.remove("PATH");

        assert_eq!(probe_bin_in(&name, &[]).resolution, BinResolution::Unknown);
        let memoized = RESOLVED_BINS.lock().contains_key(&name);
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
