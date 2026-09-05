//! The persistent Windows user PATH (`HKCU\Environment`) behind one port.
//!
//! Every read and write of that value goes through `UserPathStore`, so no
//! decision here can reach the registry on its own and tests drive the same
//! production logic against a recording store. Entry comparison follows Windows
//! PATH semantics — case-insensitive, `;`-separated, trailing separators
//! ignored — on every platform, because this value only exists on Windows.

use std::path::Path;

const SEPARATOR: &str = ";";
const REPAIR_ARG: &str = "repair-user-path";

/// Reads and writes the persistent user PATH.
pub(crate) trait UserPathStore {
    /// `None` when this platform has no persistent user PATH (no registry).
    fn read(&self) -> Option<String>;
    fn write(&self, value: &str) -> std::io::Result<()>;
}

pub(crate) struct SystemUserPathStore;

/// What the user PATH must become for `dir` to be on it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UserPathDecision {
    Append(String),
    AlreadyPresent,
    /// Under the system temp directory, so it can never be a durable install.
    RefusedEphemeral,
}

pub(crate) struct UserPathRepair {
    pub(crate) removed: Vec<String>,
    pub(crate) value: String,
}

fn path_key(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

fn trimmed(entry: &str) -> &str {
    entry.trim().trim_end_matches(['\\', '/'])
}

fn same_entry(a: &str, b: &str) -> bool {
    trimmed(a).to_lowercase() == trimmed(b).to_lowercase()
}

fn entry_text(dir: &Path) -> String {
    let text = dir.to_string_lossy().into_owned();
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

/// Components of `dir` below `ancestor`, or `None` when it is not below it.
fn components_below(dir: &Path, ancestor: &Path) -> Option<Vec<String>> {
    let (dir, ancestor) = (path_key(dir), path_key(ancestor));
    (!ancestor.is_empty() && dir.len() > ancestor.len() && dir[..ancestor.len()] == ancestor[..])
        .then(|| dir[ancestor.len()..].to_vec())
}

/// A directory at or under the system temp directory is deleted the moment its
/// owner exits, so it must never reach a persistent PATH.
pub(crate) fn is_ephemeral_install_dir(dir: &Path, temp_dir: &Path) -> bool {
    let (dir, temp) = (path_key(dir), path_key(temp_dir));
    !temp.is_empty() && dir.len() >= temp.len() && dir[..temp.len()] == temp[..]
}

/// `<system temp>\.tmp<random>\.codeterm\bin` — the shape a test run's tempdir
/// home leaves behind. Nothing else may be removed by the repair.
pub(crate) fn is_dead_temp_cli_entry(entry: &str, temp_dir: &Path) -> bool {
    let entry = trimmed(entry);
    if entry.is_empty() {
        return false;
    }
    match components_below(Path::new(entry), temp_dir).as_deref() {
        Some([tmp, data, bin]) => tmp.starts_with(".tmp") && data == ".codeterm" && bin == "bin",
        _ => false,
    }
}

pub(crate) fn user_path_after_adding(
    current: &str,
    dir: &Path,
    temp_dir: &Path,
) -> UserPathDecision {
    if is_ephemeral_install_dir(dir, temp_dir) {
        return UserPathDecision::RefusedEphemeral;
    }
    let addition = entry_text(dir);
    let present = current
        .split(SEPARATOR)
        .filter(|entry| !trimmed(entry).is_empty())
        .any(|entry| same_entry(entry, &addition));
    if present {
        return UserPathDecision::AlreadyPresent;
    }
    let kept = current.trim().trim_end_matches(SEPARATOR);
    UserPathDecision::Append(if kept.is_empty() {
        addition
    } else {
        format!("{kept}{SEPARATOR}{addition}")
    })
}

/// Every entry except the dead ones, verbatim — an untouched PATH re-serializes
/// byte for byte, which is what makes a second run a no-op.
pub(crate) fn plan_user_path_repair(current: &str, temp_dir: &Path) -> UserPathRepair {
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for entry in current.split(SEPARATOR) {
        if is_dead_temp_cli_entry(entry, temp_dir) {
            removed.push(entry.trim().to_string());
        } else {
            kept.push(entry);
        }
    }
    UserPathRepair {
        removed,
        value: kept.join(SEPARATOR),
    }
}

pub(crate) fn add_to_user_path_with(store: &dyn UserPathStore, dir: &Path, temp_dir: &Path) {
    let Some(current) = store.read() else {
        return;
    };
    match user_path_after_adding(&current, dir, temp_dir) {
        UserPathDecision::RefusedEphemeral => log::warn!(
            "[cli-install] refusing to put ephemeral {} on the user PATH",
            dir.display()
        ),
        UserPathDecision::AlreadyPresent => {}
        UserPathDecision::Append(value) => match store.write(&value) {
            Ok(()) => log::info!("[cli-install] Added {} to User PATH", dir.display()),
            Err(error) => log::error!(
                "[cli-install] Failed to add {} to User PATH: {}",
                dir.display(),
                error
            ),
        },
    }
}

pub(crate) fn add_to_user_path(dir: &Path) {
    add_to_user_path_with(&SystemUserPathStore, dir, &std::env::temp_dir());
}

pub(crate) fn repair_user_path_with(
    store: &dyn UserPathStore,
    temp_dir: &Path,
) -> std::io::Result<Vec<String>> {
    let Some(current) = store.read() else {
        return Ok(Vec::new());
    };
    let repair = plan_user_path_repair(&current, temp_dir);
    if repair.removed.is_empty() {
        return Ok(Vec::new());
    }
    store.write(&repair.value)?;
    Ok(repair.removed)
}

pub(crate) fn user_path_repair_report(removed: &[String]) -> String {
    let mut report = format!(
        "CodeTerm user PATH repair: removed {} dead entries\n",
        removed.len()
    );
    for entry in removed {
        report.push_str("  - ");
        report.push_str(entry);
        report.push('\n');
    }
    report
}

/// `CodeTerm repair-user-path` — never automatic, never part of startup. The
/// GUI binary has no console on Windows, so the report is also written to a
/// file whose path the caller is told.
pub(crate) fn run_user_path_repair_if_invoked() {
    if std::env::args().nth(1).as_deref() != Some(REPAIR_ARG) {
        return;
    }
    let (report, code) = match repair_user_path_with(&SystemUserPathStore, &std::env::temp_dir()) {
        Ok(removed) => (user_path_repair_report(&removed), 0),
        Err(error) => (format!("CodeTerm user PATH repair failed: {error}\n"), 1),
    };
    let report_path = std::env::temp_dir().join("codeterm-user-path-repair.txt");
    let _ = std::fs::write(&report_path, &report);
    use std::io::Write;
    let _ = writeln!(
        std::io::stdout(),
        "{report}Report written to {}",
        report_path.display()
    );
    std::process::exit(code);
}

#[cfg(windows)]
fn parse_reg_query_path(raw: &str) -> String {
    raw.lines()
        .find(|line| line.contains("REG_") && line.contains("Path"))
        .and_then(|line| {
            line.split("REG_EXPAND_SZ")
                .nth(1)
                .or_else(|| line.split("REG_SZ").nth(1))
        })
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

impl UserPathStore for SystemUserPathStore {
    #[cfg(windows)]
    fn read(&self) -> Option<String> {
        let mut query = std::process::Command::new("reg");
        query.args(["query", "HKCU\\Environment", "/v", "Path"]);
        crate::process_util::no_window_std(&mut query);
        let raw = match query.output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
            Err(_) => String::new(),
        };
        Some(parse_reg_query_path(&raw))
    }

    #[cfg(not(windows))]
    fn read(&self) -> Option<String> {
        None
    }

    #[cfg(windows)]
    fn write(&self, value: &str) -> std::io::Result<()> {
        let mut add = std::process::Command::new("reg");
        add.args([
            "add",
            "HKCU\\Environment",
            "/v",
            "Path",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            value,
            "/f",
        ]);
        crate::process_util::no_window_std(&mut add);
        let output = add.output()?;
        if output.status.success() {
            return Ok(());
        }
        Err(std::io::Error::other(format!(
            "reg add HKCU Environment Path failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    #[cfg(not(windows))]
    fn write(&self, _value: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this platform has no persistent user PATH",
        ))
    }
}

#[cfg(test)]
mod tests;
