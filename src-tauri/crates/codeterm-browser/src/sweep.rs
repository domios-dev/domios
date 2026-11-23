use super::profile::{ProfileLayout, PROFILES_DIR, SESSIONS_DIR};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const USER_DATA_DIR: &str = "--user-data-dir=";
const INVENTORY_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRow {
    pub pid: u32,
    pub parent_pid: u32,
    pub command_line: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanMatch {
    pub pid: u32,
    pub parent_pid: u32,
    pub profile_dir: PathBuf,
}

#[derive(Debug, Default)]
pub struct SweepReport {
    pub killed: Vec<u32>,
    pub removed_dirs: Vec<PathBuf>,
    pub errors: Vec<String>,
}

pub fn paths_case_insensitive() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// Byte-for-byte mapping (separators, ASCII case) so offsets stay valid in the original text.
fn normalize(text: &str, case_insensitive: bool) -> String {
    text.chars()
        .map(|c| match c {
            '\\' => '/',
            c if case_insensitive => c.to_ascii_lowercase(),
            c => c,
        })
        .collect()
}

fn normalized_dir(path: &Path, case_insensitive: bool) -> String {
    normalize(&path.to_string_lossy(), case_insensitive)
        .trim_end_matches('/')
        .to_string()
}

/// The `<root>/<sessions|profiles>/<name>` dir a Chromium command line runs on, if it is ours.
pub fn profile_dir_in_command_line(
    command_line: &str,
    root: &Path,
    case_insensitive: bool,
) -> Option<PathBuf> {
    let root_norm = normalized_dir(root, case_insensitive);
    let line_norm = normalize(command_line, case_insensitive);
    let mut search_from = 0;
    while let Some(found) = line_norm[search_from..].find(USER_DATA_DIR) {
        let flag_start = search_from + found;
        let value_start = flag_start + USER_DATA_DIR.len();
        search_from = value_start;
        let before_flag = line_norm[..flag_start]
            .strip_suffix('"')
            .unwrap_or(&line_norm[..flag_start]);
        if before_flag
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace())
        {
            continue;
        }
        let value = line_norm[value_start..].trim_start_matches('"');
        let Some(below_root) = value.strip_prefix(root_norm.as_str()) else {
            continue;
        };
        let Some(below_root) = below_root.strip_prefix('/') else {
            continue;
        };
        let mut parts = below_root.splitn(2, '/');
        let kind = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        let name_len = rest
            .find(|c: char| c == '/' || c == '"' || c.is_whitespace())
            .unwrap_or(rest.len());
        let kind = [SESSIONS_DIR, PROFILES_DIR]
            .into_iter()
            .find(|dir| normalize(dir, case_insensitive) == kind);
        if let (Some(kind), true) = (kind, name_len > 0 && !rest[name_len..].starts_with('/')) {
            let name_start = line_norm.len() - rest.len();
            let name = &command_line[name_start..name_start + name_len];
            if super::profile::sanitize_profile_name(name) == name {
                return Some(root.join(kind).join(name));
            }
        }
    }
    None
}

pub fn find_orphans(
    rows: &[ProcessRow],
    root: &Path,
    own_pid: u32,
    keep: &HashSet<PathBuf>,
    case_insensitive: bool,
) -> Vec<OrphanMatch> {
    let kept: HashSet<String> = keep
        .iter()
        .map(|dir| normalized_dir(dir, case_insensitive))
        .collect();
    rows.iter()
        .filter(|row| row.pid != own_pid && row.pid > 1)
        .filter_map(|row| {
            let profile_dir = owned_profile(row, root, case_insensitive)?;
            (!kept.contains(&normalized_dir(&profile_dir, case_insensitive))).then_some(
                OrphanMatch {
                    pid: row.pid,
                    parent_pid: row.parent_pid,
                    profile_dir,
                },
            )
        })
        .collect()
}

#[cfg(windows)]
fn owned_profile(row: &ProcessRow, root: &Path, case_insensitive: bool) -> Option<PathBuf> {
    let args = codeterm_util::process_util::windows_command_line_args(&row.command_line)?;
    let args = args.get(1..)?;
    if !args.iter().any(|arg| arg == super::launch::OWNERSHIP_FLAG) {
        return None;
    }
    let mut profiles = args.iter().filter(|arg| arg.starts_with(USER_DATA_DIR));
    let arg = profiles.next()?;
    if profiles.next().is_some() {
        return None;
    }
    let value = arg.strip_prefix(USER_DATA_DIR)?;
    let normalized = normalize(value, case_insensitive);
    let root_norm = normalized_dir(root, case_insensitive);
    let below_root = normalized
        .strip_prefix(root_norm.as_str())?
        .strip_prefix('/')?;
    let (kind, name) = below_root.split_once('/')?;
    let kind = [SESSIONS_DIR, PROFILES_DIR]
        .into_iter()
        .find(|dir| normalize(dir, case_insensitive) == kind)?;
    let name = &value[value.len() - name.len()..];
    (!name.is_empty() && super::profile::sanitize_profile_name(name) == name)
        .then(|| root.join(kind).join(name))
}

#[cfg(unix)]
fn owned_profile(row: &ProcessRow, root: &Path, case_insensitive: bool) -> Option<PathBuf> {
    row.command_line
        .split_whitespace()
        .any(|arg| arg == super::launch::OWNERSHIP_FLAG)
        .then(|| profile_dir_in_command_line(&row.command_line, root, case_insensitive))
        .flatten()
}

/// Tree kills start at matches whose parent is not itself a match, so each tree is killed once.
pub fn orphan_roots(orphans: &[OrphanMatch]) -> Vec<u32> {
    let pids: HashSet<u32> = orphans.iter().map(|orphan| orphan.pid).collect();
    orphans
        .iter()
        .filter(|orphan| !pids.contains(&orphan.parent_pid))
        .map(|orphan| orphan.pid)
        .collect()
}

/// `ps -axww -o pid=,ppid=,args=` output.
pub fn parse_ps_output(stdout: &str) -> Vec<ProcessRow> {
    stdout
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (parent_pid, command_line) = rest.trim_start().split_once(char::is_whitespace)?;
            Some(ProcessRow {
                pid: pid.parse().ok()?,
                parent_pid: parent_pid.parse().ok()?,
                command_line: command_line.trim().to_string(),
            })
        })
        .collect()
}

/// `ConvertTo-Json` of `ProcessId,ParentProcessId,CommandLine`; one result serializes as an object, not an array.
pub fn parse_cim_json(stdout: &str) -> Result<Vec<ProcessRow>, String> {
    let trimmed = stdout.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let rows = match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Array(rows)) => rows,
        Ok(row @ Value::Object(_)) => vec![row],
        _ => return Err("invalid browser process inventory".into()),
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some(ProcessRow {
                pid: u32::try_from(row.get("ProcessId")?.as_u64()?).ok()?,
                parent_pid: row
                    .get("ParentProcessId")
                    .and_then(Value::as_u64)
                    .and_then(|pid| u32::try_from(pid).ok())
                    .unwrap_or(0),
                command_line: row.get("CommandLine")?.as_str()?.to_string(),
            })
        })
        .collect())
}

#[cfg(windows)]
fn enumerate_processes() -> Result<Vec<ProcessRow>, String> {
    let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
        Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*--user-data-dir=*' } | \
        Select-Object ProcessId,ParentProcessId,CommandLine | ConvertTo-Json -Compress";
    let output = codeterm_util::process_util::bounded_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
        INVENTORY_DEADLINE,
    )
    .map_err(|error| format!("process enumeration failed: {error}"))?;
    parse_cim_json(&output)
}

#[cfg(not(windows))]
fn enumerate_processes() -> Result<Vec<ProcessRow>, String> {
    let output = codeterm_util::process_util::bounded_capture(
        "ps",
        &["-axww", "-o", "pid=,ppid=,args="],
        INVENTORY_DEADLINE,
    )
    .map_err(|error| format!("process enumeration failed: {error}"))?;
    Ok(parse_ps_output(&output))
}

#[cfg(windows)]
fn reap_orphan(
    pid: u32,
    identity: Option<&codeterm_util::process_tree::ProcessRecord>,
) -> Result<(), String> {
    use codeterm_util::process_tree::OwnedProcessTree;
    let identity = identity.ok_or_else(|| format!("no browser launch identity for PID {pid}"))?;
    let executable = identity
        .executable
        .as_ref()
        .ok_or_else(|| format!("no browser image for PID {pid}"))?;
    let tree = OwnedProcessTree::capture_including_root(
        (pid, identity.created),
        std::slice::from_ref(executable),
    )
    .map_err(|error| format!("browser PID {pid} identity changed: {error}"))?;
    tree.terminate_remaining(super::process::KILL_DEADLINE)
        .map_err(|error| format!("browser PID {pid} tree exit: {error}"))
}

#[cfg(unix)]
fn reap_orphan(
    orphan: &OrphanMatch,
    layout: &ProfileLayout,
    case_insensitive: bool,
) -> Result<(), String> {
    let pid = i32::try_from(orphan.pid).map_err(|_| "browser PID out of range".to_string())?;
    let current = codeterm_util::process_util::bounded_capture(
        "ps",
        &["-p", &pid.to_string(), "-ww", "-o", "pid=,ppid=,args="],
        INVENTORY_DEADLINE,
    )
    .map_err(|error| format!("browser PID {pid} no longer identifiable: {error}"))?;
    if !parse_ps_output(&current).iter().any(|row| {
        row.pid == orphan.pid
            && owned_profile(row, layout.root(), case_insensitive).as_ref()
                == Some(&orphan.profile_dir)
    }) {
        return Err(format!("browser PID {pid} launch identity changed"));
    };
    let group = unsafe { libc::getpgid(pid) };
    let target = if group == pid { -pid } else { pid };
    if unsafe { libc::kill(target, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("kill {pid}: {error}"))
    }
}

/// Blocking startup sweep: kill browsers left on our profile root, then drop stale throwaway dirs.
pub fn sweep_orphans_blocking(layout: &ProfileLayout, keep: &HashSet<PathBuf>) -> SweepReport {
    let mut report = SweepReport::default();
    let case_insensitive = paths_case_insensitive();
    let mut kept: HashSet<String> = keep
        .iter()
        .map(|dir| normalized_dir(dir, case_insensitive))
        .collect();
    #[cfg(windows)]
    let process_identities = codeterm_util::process_tree::process_table();
    match enumerate_processes() {
        Ok(rows) => {
            let orphans = find_orphans(
                &rows,
                layout.root(),
                std::process::id(),
                keep,
                case_insensitive,
            );
            let orphan_ids: HashSet<_> = orphans.iter().map(|orphan| orphan.pid).collect();
            kept.extend(
                rows.iter()
                    .filter(|row| {
                        !orphan_ids.contains(&row.pid) && !row.command_line.contains(" --type=")
                    })
                    .filter_map(|row| {
                        let dir = profile_dir_in_command_line(
                            &row.command_line,
                            layout.root(),
                            case_insensitive,
                        )?;
                        Some(normalized_dir(&dir, case_insensitive))
                    }),
            );
            let roots = orphan_roots(&orphans);
            for orphan in orphans.iter().filter(|orphan| roots.contains(&orphan.pid)) {
                let pid = orphan.pid;
                #[cfg(windows)]
                let result = match &process_identities {
                    Ok(identities) => reap_orphan(pid, identities.get(&pid)),
                    Err(error) => Err(format!("browser process identities unavailable: {error}")),
                };
                #[cfg(unix)]
                let result = reap_orphan(orphan, layout, case_insensitive);
                match result {
                    Ok(()) => report.killed.push(pid),
                    Err(error) => {
                        kept.insert(normalized_dir(&orphan.profile_dir, case_insensitive));
                        report.errors.push(error);
                    }
                }
            }
        }
        Err(error) => {
            log::warn!("[browser-sweep] process enumeration failed: {error}");
            report.errors.push(error);
            return report;
        }
    }
    let sessions = layout.sessions_dir();
    if codeterm_util::safe_tree::refuse_linked_ancestors(&sessions).is_ok() {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if kept.contains(&normalized_dir(&dir, case_insensitive))
                    || !entry.file_type().is_ok_and(|kind| kind.is_dir())
                    || !entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok())
                    || codeterm_util::safe_tree::refuse_linked_ancestors(&dir).is_err()
                {
                    continue;
                }
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => report.removed_dirs.push(dir),
                    Err(error) => report
                        .errors
                        .push(format!("remove {}: {error}", dir.display())),
                }
            }
        }
    }
    if !report.killed.is_empty() || !report.errors.is_empty() {
        log::warn!(
            "[browser-sweep] killed {:?}, removed {} stale profiles, errors {:?}",
            report.killed,
            report.removed_dirs.len(),
            report.errors
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, parent_pid: u32, command_line: &str) -> ProcessRow {
        ProcessRow {
            pid,
            parent_pid,
            command_line: command_line.into(),
        }
    }

    #[test]
    fn browser_sweep_matches_windows_renderer_with_quoted_value() {
        let root = Path::new(r"C:\Users\dev\.domios-dev\browsers");
        let line = r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --type=renderer --user-data-dir="C:\Users\dev\.domios-dev\browsers\sessions\5c1a3b9e" --lang=en-US"#;
        assert_eq!(
            profile_dir_in_command_line(line, root, true),
            Some(root.join("sessions").join("5c1a3b9e"))
        );
    }

    #[test]
    fn browser_sweep_matches_whole_arg_quoted_path_with_spaces() {
        let root = Path::new(r"C:\Users\User Name\.domios\browsers");
        let line = r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --headless=new "--user-data-dir=c:\users\user name\.domios\browsers\profiles\Work" about:blank"#;
        assert_eq!(
            profile_dir_in_command_line(line, root, true),
            Some(root.join("profiles").join("Work"))
        );
    }

    #[test]
    fn browser_sweep_matches_unquoted_posix_command_line() {
        let root = Path::new("/home/u/.domios/browsers");
        let line = "/opt/google/chrome/chrome --type=gpu-process --user-data-dir=/home/u/.domios/browsers/sessions/abc123 --enable-crash-reporter";
        assert_eq!(
            profile_dir_in_command_line(line, root, false),
            Some(root.join("sessions").join("abc123"))
        );
    }

    #[test]
    fn browser_sweep_ignores_foreign_and_lookalike_profiles() {
        let root = Path::new("/home/u/.domios/browsers");
        for line in [
            "/usr/bin/google-chrome --user-data-dir=/home/u/.config/google-chrome",
            "/usr/bin/google-chrome --user-data-dir=/home/u/.domios/browsers-old/sessions/x",
            "/usr/bin/google-chrome --user-data-dir=/home/u/.domios/browsers/other/x",
            "/usr/bin/google-chrome --user-data-dir=/home/u/.domios/browsers/sessions/",
            "/usr/bin/google-chrome /home/u/.domios/browsers/sessions/x",
            "/usr/bin/google-chrome --user-data-dir=/HOME/U/.domios/browsers/sessions/x",
            "/usr/bin/google-chrome --user-data-dir=/home/u/.domios/browsers/sessions/../x",
            "/usr/bin/google-chrome --user-data-dir=/home/u/.domios/browsers/sessions/x/subdir",
            "/usr/bin/google-chrome --description=--user-data-dir=/home/u/.domios/browsers/sessions/x",
        ] {
            assert_eq!(
                profile_dir_in_command_line(line, root, false),
                None,
                "{line}"
            );
        }
    }

    #[test]
    fn browser_sweep_find_orphans_skips_own_pid_and_kept_profiles() {
        let root = Path::new("/d/browsers");
        let rows = vec![
            row(10, 1, "chrome --codeterm-owned-browser --user-data-dir=/d/browsers/sessions/live"),
            row(11, 1, "chrome --codeterm-owned-browser --user-data-dir=/d/browsers/sessions/dead"),
            row(
                13,
                11,
                "chrome --codeterm-owned-browser --type=renderer --user-data-dir=/d/browsers/sessions/dead",
            ),
            row(42, 1, "codeterm --user-data-dir=/d/browsers/sessions/dead"),
            row(12, 1, "bash -c true"),
        ];
        let keep: HashSet<PathBuf> = [root.join("sessions").join("live")].into_iter().collect();
        let orphans = find_orphans(&rows, root, 42, &keep, false);
        assert_eq!(
            orphans.iter().map(|orphan| orphan.pid).collect::<Vec<_>>(),
            vec![11, 13]
        );
        assert_eq!(orphans[0].profile_dir, root.join("sessions").join("dead"));
        assert_eq!(orphan_roots(&orphans), vec![11]);
    }

    #[test]
    fn browser_sweep_keeps_foreign_profile_processes() {
        let root = Path::new("/d/browsers");
        let rows = vec![row(
            90,
            1,
            "chrome --user-data-dir=/d/browsers/profiles/work",
        )];
        assert!(find_orphans(&rows, root, 42, &HashSet::new(), false).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn browser_sweep_ignores_ownership_flags_inside_windows_arguments() {
        let root = Path::new(r"C:\Users\Fixture User\.domios\browsers");
        let rows = vec![row(
            90,
            1,
            r#"powershell.exe -Command "echo --codeterm-owned-browser --user-data-dir=C:\Users\Fixture User\.domios\browsers\profiles\work""#,
        )];
        assert!(find_orphans(&rows, root, 42, &HashSet::new(), true).is_empty());
    }

    #[test]
    fn browser_sweep_parses_ps_output() {
        let stdout =
            "    1     0 /sbin/init\n  4321  4000 /usr/bin/chromium --user-data-dir=/x y\n\nbogus line\n";
        assert_eq!(
            parse_ps_output(stdout),
            vec![
                row(1, 0, "/sbin/init"),
                row(4321, 4000, "/usr/bin/chromium --user-data-dir=/x y")
            ]
        );
    }

    #[test]
    fn browser_sweep_parses_cim_json_array_object_and_empty() {
        let array = r#"[{"ProcessId":7,"ParentProcessId":3,"CommandLine":"chrome.exe --user-data-dir=\"C:\\x\""},{"ProcessId":8,"CommandLine":null}]"#;
        assert_eq!(
            parse_cim_json(array).unwrap(),
            vec![row(7, 3, r#"chrome.exe --user-data-dir="C:\x""#)]
        );
        let object = "\u{feff}{\"ProcessId\":9,\"CommandLine\":\"msedge.exe\"}\r\n";
        assert_eq!(
            parse_cim_json(object).unwrap(),
            vec![row(9, 0, "msedge.exe")]
        );
        assert!(parse_cim_json("").unwrap().is_empty());
        assert!(parse_cim_json("not json").is_err());
    }
}
