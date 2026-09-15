use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use codeterm_types::plan::{LaneDiagnosticFinding, LaneDiagnosticsSummary};
use parking_lot::Mutex;
use serde_json::Value;
use wait_timeout::ChildExt;

use crate::process_util;

pub(crate) const DIAGNOSTICS_DEADLINE: Duration = Duration::from_secs(900);
const PREPARE_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnalysisScope {
    Canonical,
    Warm,
    WorkerLane,
    Unknown,
}

pub(crate) fn analysis_scope(
    canonical: &str,
    warm: &str,
    lane: Option<&str>,
    lane_marker: Option<&str>,
    checkout: &str,
) -> AnalysisScope {
    if super::same_path(canonical, checkout) {
        return AnalysisScope::Canonical;
    }
    if super::same_path(warm, checkout) {
        return AnalysisScope::Warm;
    }
    if lane_marker.is_some_and(|marker| {
        lane.is_some_and(|record| super::same_path(record, marker))
            && super::same_path(marker, checkout)
    }) {
        return AnalysisScope::WorkerLane;
    }
    AnalysisScope::Unknown
}

pub(crate) fn analysis_allowed(scope: AnalysisScope) -> bool {
    matches!(scope, AnalysisScope::Canonical | AnalysisScope::Warm)
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticsRequest {
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    pub(crate) attempt_id: String,
    pub(crate) canonical_project_path: String,
    pub(crate) lane_worktree_path: String,
    pub(crate) lane_marker: Option<String>,
    pub(crate) lane_branch: Option<String>,
    pub(crate) base: Option<String>,
    pub(crate) sha: Option<String>,
    pub(crate) changed_files: Vec<String>,
    pub(crate) require_lane_marker: bool,
}

pub(crate) struct LaneDiagnosticsService {
    storage_root: PathBuf,
    run_lock: Mutex<()>,
    deadline: Duration,
}

impl LaneDiagnosticsService {
    pub(crate) fn new(storage_root: PathBuf) -> Arc<Self> {
        Self::with_deadline(storage_root, DIAGNOSTICS_DEADLINE)
    }

    pub(crate) fn with_deadline(storage_root: PathBuf, deadline: Duration) -> Arc<Self> {
        Arc::new(Self {
            storage_root,
            run_lock: Mutex::new(()),
            deadline,
        })
    }

    pub(crate) fn run(
        &self,
        request: DiagnosticsRequest,
    ) -> Result<LaneDiagnosticsSummary, String> {
        let _run_guard = self.run_lock.lock();
        let warm = warm_worktree_path(&request.canonical_project_path)?;
        let lane_scope = analysis_scope(
            &request.canonical_project_path,
            &warm,
            Some(&request.lane_worktree_path),
            request.lane_marker.as_deref(),
            &request.lane_worktree_path,
        );
        if request.require_lane_marker && lane_scope != AnalysisScope::WorkerLane {
            return Err("lane diagnostics refused: task lane and dispatcher marker do not agree".into());
        }
        if super::same_path(&request.canonical_project_path, &request.lane_worktree_path) {
            return Err("lane diagnostics refused: lane path is the canonical checkout".into());
        }
        if super::same_path(&warm, &request.lane_worktree_path) {
            return Err("lane diagnostics refused: lane path is the diagnostics worktree".into());
        }
        let lane_tip = git_value(&request.lane_worktree_path, &["rev-parse", "HEAD"])?;
        if let Some(reported) = request
            .sha
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let reported_commit = format!("{reported}^{{commit}}");
            let resolved = git_value(
                &request.lane_worktree_path,
                &["rev-parse", "--verify", "--end-of-options", &reported_commit],
            )?;
            if resolved != lane_tip {
                return Err("lane diagnostics refused: report SHA is not the current lane tip".into());
            }
        }
        prepare_warm_worktree(&request.canonical_project_path, &warm, Some(&lane_tip))?;
        if !analysis_allowed(analysis_scope(
            &request.canonical_project_path,
            &warm,
            Some(&request.lane_worktree_path),
            None,
            &warm,
        )) {
            return Err("lane diagnostics refused: warm checkout is not an allowed analysis scope".into());
        }
        let changed_files = changed_files(&request, &lane_tip);
        let crates = touched_crates(&changed_files);
        let target = warm.join(".codeterm-diagnostics-target");
        let mut command = bash_command(&warm);
        command
            .arg("build.sh")
            .arg("lane-diagnostics")
            .current_dir(&warm)
            .env("CI", "true")
            .env("CODETERM_DIAGNOSTICS_OWNER", "daemon")
            .env("CODETERM_DIAGNOSTICS_SCOPE", "warm")
            .env("CODETERM_DIAGNOSTICS_CRATES", crates.join(","))
            .env("CODETERM_DIAGNOSTICS_FILES", changed_files.join("\n"))
            .env(
                "CODETERM_DIAGNOSTICS_BRANCH",
                request.lane_branch.clone().unwrap_or_default(),
            )
            .env("CODETERM_DIAGNOSTICS_BASE", request.base.clone().unwrap_or_default())
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let execution = run_command(command, self.deadline)?;
        if execution.timed_out {
            let summary = LaneDiagnosticsSummary::incomplete(
                request.plan_id,
                request.task_id,
                request.attempt_id,
                warm.display().to_string(),
                "diagnostics incomplete: deadline exceeded".into(),
            );
            self.store_summary(&summary)?;
            return Ok(summary);
        }
        let mut findings = parse_diagnostics(&execution.output, &changed_files);
        if execution.code != Some(0) && findings.is_empty() {
            findings.push(LaneDiagnosticFinding {
                tool: "lane-diagnostics".into(),
                severity: "error".into(),
                message: format!(
                    "diagnostics runner exited with status {}",
                    execution
                        .code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".into())
                ),
                ..Default::default()
            });
        }
        let mut summary = LaneDiagnosticsSummary::from_findings(
            request.plan_id,
            request.task_id,
            request.attempt_id,
            warm.display().to_string(),
            diagnostic_tools(&execution.output),
            findings,
        );
        if execution.code != Some(0) {
            summary.status = "incomplete".into();
            summary.incomplete = true;
        }
        self.store_summary(&summary)?;
        Ok(summary)
    }

    pub(crate) fn load(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
    ) -> Result<LaneDiagnosticsSummary, String> {
        let path = self.storage_path(plan_id, task_id, attempt_id);
        let json = std::fs::read_to_string(&path)
            .map_err(|error| format!("diagnostics not found at {}: {error}", path.display()))?;
        serde_json::from_str(&json).map_err(|error| format!("invalid diagnostics JSON: {error}"))
    }

    pub(crate) fn store_summary(&self, summary: &LaneDiagnosticsSummary) -> Result<(), String> {
        std::fs::create_dir_all(&self.storage_root).map_err(|error| {
            format!(
                "cannot create diagnostics store {}: {error}",
                self.storage_root.display()
            )
        })?;
        let path = self.storage_path(&summary.plan_id, &summary.task_id, &summary.attempt_id);
        let temp = path.with_file_name(format!(".{}.{}.tmp", path.file_name().unwrap().to_string_lossy(), uuid::Uuid::new_v4().simple()));
        let json = serde_json::to_vec_pretty(summary)
            .map_err(|error| format!("cannot serialize diagnostics: {error}"))?;
        std::fs::write(&temp, json)
            .map_err(|error| format!("cannot write diagnostics {}: {error}", temp.display()))?;
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| {
                format!("cannot replace diagnostics {}: {error}", path.display())
            })?;
        }
        std::fs::rename(&temp, &path).map_err(|error| {
            format!("cannot commit diagnostics {}: {error}", path.display())
        })
    }

    fn storage_path(&self, plan_id: &str, task_id: &str, attempt_id: &str) -> PathBuf {
        self.storage_root.join(format!(
            "{}_{}_{}.json",
            safe_component(plan_id),
            safe_component(task_id),
            safe_component(attempt_id)
        ))
    }
}

fn safe_component(value: &str) -> String {
    let encoded: String = value
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || matches!(char, '-' | '_') {
                char
            } else {
                '_'
            }
        })
        .collect();
    if encoded.is_empty() {
        "unknown".into()
    } else {
        encoded
    }
}

pub(crate) fn warm_worktree_path(canonical: &str) -> Result<PathBuf, String> {
    let project = Path::new(canonical);
    let repo = project
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("cannot derive diagnostics worktree from '{canonical}'"))?;
    let parent = project
        .parent()
        .ok_or_else(|| format!("cannot derive diagnostics worktree from '{canonical}'"))?;
    Ok(parent.join(format!("{repo}-worktrees")).join(".diagnostics"))
}

fn prepare_warm_worktree(canonical: &str, warm: &Path, sha: Option<&str>) -> Result<(), String> {
    let tip = sha
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| git_value(canonical, &["rev-parse", "HEAD"]).ok())
        .ok_or_else(|| "diagnostics could not resolve a checkout tip".to_string())?;
    if !warm.exists() {
        if let Some(parent) = warm.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("cannot create diagnostics worktree parent {}: {error}", parent.display())
            })?;
        }
        let warm_display = warm.display().to_string();
        let result = run_command(
            git_command(canonical, &["worktree", "add", "--detach", &warm_display, &tip]),
            PREPARE_DEADLINE,
        )?;
        return result_success(result, "create diagnostics warm worktree");
    }
    if !warm.is_dir() {
        return Err(format!(
            "diagnostics worktree path is occupied by a non-directory: {}",
            warm.display()
        ));
    }
    let warm_str = warm
        .to_str()
        .ok_or_else(|| format!("diagnostics worktree path is not valid Unicode: {}", warm.display()))?;
    let top = git_value(warm_str, &["rev-parse", "--show-toplevel"])?;
    if !super::same_path(&top, &warm.display().to_string()) {
        return Err(format!(
            "diagnostics worktree path is occupied by another checkout: {}",
            warm.display()
        ));
    }
    let result = run_command(
        git_command(warm_str, &["reset", "--hard", &tip]),
        PREPARE_DEADLINE,
    )?;
    result_success(result, "refresh diagnostics warm worktree")
}

fn result_success(result: ProcessOutput, operation: &str) -> Result<(), String> {
    if result.timed_out {
        return Err(format!("{operation} exceeded its deadline"));
    }
    if result.code == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "{operation} failed: {}",
            result.output.trim()
        ))
    }
}

fn git_command(cwd: &str, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    process_util::no_window_std(&mut command);
    command.current_dir(cwd).args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

fn bash_command(_cwd: &Path) -> Command {
    let bash = which::which("bash").unwrap_or_else(|_| PathBuf::from("bash"));
    let mut command = Command::new(bash);
    process_util::no_window_std(&mut command);
    configure_process_group(&mut command);
    command
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[derive(Debug)]
struct ProcessOutput {
    code: Option<i32>,
    timed_out: bool,
    output: String,
}

fn run_command(mut command: Command, deadline: Duration) -> Result<ProcessOutput, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start diagnostics command: {error}"))?;
    let stdout = child.stdout.take().map(read_pipe);
    let stderr = child.stderr.take().map(read_pipe);
    let timed_out = child
        .wait_timeout(deadline)
        .map_err(|error| format!("diagnostics wait failed: {error}"))?
        .is_none();
    if timed_out {
        let _ = kill_child(&mut child);
    }
    let code = child
        .wait()
        .map_err(|error| format!("diagnostics command wait failed: {error}"))?
        .code();
    let mut output = stdout
        .map(|thread| thread.join().unwrap_or_default())
        .unwrap_or_default();
    output.push_str(&stderr.map(|thread| thread.join().unwrap_or_default()).unwrap_or_default());
    Ok(ProcessOutput {
        code,
        timed_out,
        output,
    })
}

fn read_pipe<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = reader.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn kill_child(child: &mut Child) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let mut command = Command::new("taskkill");
        process_util::no_window_std(&mut command);
        if let Ok(status) = command.args(["/PID", &pid, "/T", "/F"]).status() {
            if status.success() {
                return Ok(());
            }
        }
    }
    #[cfg(unix)]
    {
        let process_group = -(child.id() as libc::pid_t);
        if unsafe { libc::kill(process_group, libc::SIGKILL) } == 0 {
            return Ok(());
        }
    }
    child.kill()
}

fn git_value(cwd: &str, args: &[&str]) -> Result<String, String> {
    let result = run_command(git_command(cwd, args), PREPARE_DEADLINE)?;
    if result.timed_out {
        return Err("git query exceeded its deadline".into());
    }
    if result.code != Some(0) {
        return Err(format!("git query failed: {}", result.output.trim()));
    }
    Ok(result.output.trim().to_string())
}

pub(crate) fn touched_crates(files: &[String]) -> Vec<String> {
    let mut crates = Vec::new();
    let mut add = |name: &str| {
        if !crates.iter().any(|item| item == name) {
            crates.push(name.to_string());
        }
    };
    for file in files {
        if file.starts_with("cli/") {
            add("cli");
        } else if file.starts_with("src-tauri/crates/codeterm-plan/") {
            add("plan");
        } else if file.starts_with("src-tauri/crates/codeterm-types/") {
            add("types");
        } else if file.starts_with("src-tauri/") {
            add("app");
        }
    }
    crates
}

fn changed_files(request: &DiagnosticsRequest, sha: &str) -> Vec<String> {
    if !request.changed_files.is_empty() {
        return request.changed_files.clone();
    }
    let Some(base) = request.base.as_deref().filter(|value| !value.trim().is_empty()) else {
        return Vec::new();
    };
    git_value(
        &request.lane_worktree_path,
        &["diff", "--name-only", &format!("{base}...{sha}")],
    )
    .map(|value| value.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect())
    .unwrap_or_default()
}

fn diagnostic_tools(output: &str) -> Vec<String> {
    let mut tools = Vec::new();
    for line in output.lines() {
        let Some(tool) = line.strip_prefix("CODETERM_DIAGNOSTIC_TOOL\t") else {
            continue;
        };
        if !tools.iter().any(|item| item == tool) {
            tools.push(tool.to_string());
        }
    }
    tools
}

pub(crate) fn parse_diagnostics(output: &str, changed_files: &[String]) -> Vec<LaneDiagnosticFinding> {
    let mut findings = Vec::new();
    let mut tool = "lane-diagnostics".to_string();
    let mut exits = Vec::new();
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("CODETERM_DIAGNOSTIC_TOOL\t") {
            tool = value.trim().to_string();
            continue;
        }
        if let Some(value) = line.strip_prefix("CODETERM_DIAGNOSTIC_EXIT\t") {
            if let Some((_, code)) = value.split_once('\t') {
                exits.push((tool.clone(), code.parse::<i32>().unwrap_or(1)));
            }
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if let Some(finding) = cargo_finding(&tool, &value) {
                findings.push(finding);
                continue;
            }
        }
        if let Some(finding) = text_finding(&tool, line) {
            findings.push(finding);
        }
    }
    for (exit_tool, code) in exits {
        if code != 0 && !findings.iter().any(|finding| finding.tool == exit_tool) {
            findings.push(LaneDiagnosticFinding {
                tool: exit_tool,
                severity: "error".into(),
                file: changed_files.first().cloned(),
                message: format!("command exited with status {code}"),
                ..Default::default()
            });
        }
    }
    findings.retain(|finding| {
        finding.file.as_deref().is_none()
            || changed_files.is_empty()
            || changed_files
                .iter()
                .any(|changed| diagnostic_file_matches(finding.file.as_deref().unwrap(), changed))
    });
    findings
}

fn diagnostic_file_matches(file: &str, changed: &str) -> bool {
    let file = file
        .trim()
        .trim_start_matches("file://")
        .replace('\\', "/");
    let changed = changed.trim().trim_start_matches("./").replace('\\', "/");
    let matches = |candidate: &str| {
        file == candidate || file.ends_with(&format!("/{candidate}"))
    };
    if matches(&changed) {
        return true;
    }
    let Some(repo_relative) = changed.strip_prefix("src-tauri/") else {
        return false;
    };
    if matches(repo_relative) {
        return true;
    }
    repo_relative
        .strip_prefix("crates/")
        .and_then(|value| value.split_once('/'))
        .is_some_and(|(_, source)| matches(source))
}

fn cargo_finding(tool: &str, value: &Value) -> Option<LaneDiagnosticFinding> {
    if !matches!(value.get("reason").and_then(Value::as_str), Some("compiler-message")) {
        return None;
    }
    let message = value.get("message")?;
    let severity = match message.get("level").and_then(Value::as_str) {
        Some("error") => "error",
        Some("warning") => "warning",
        Some("note") | Some("help") => "info",
        _ => return None,
    };
    let span = message
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| spans.iter().find(|span| span.get("is_primary") == Some(&Value::Bool(true))).or_else(|| spans.first()));
    Some(LaneDiagnosticFinding {
        tool: tool.to_string(),
        severity: severity.into(),
        file: span.and_then(|span| span.get("file_name")).and_then(Value::as_str).map(str::to_string),
        line: span.and_then(|span| span.get("line_start")).and_then(Value::as_u64).map(|value| value as u32),
        column: span.and_then(|span| span.get("column_start")).and_then(Value::as_u64).map(|value| value as u32),
        message: message.get("message").and_then(Value::as_str).unwrap_or_default().to_string(),
        code: message.get("code").and_then(|code| code.get("code")).and_then(Value::as_str).map(str::to_string),
    })
}

fn text_finding(tool: &str, line: &str) -> Option<LaneDiagnosticFinding> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("Build gateway") || trimmed.starts_with("[cache-policy]") {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("Diff in ") {
        let (file, line_number) = rest.split_once(" at line ")?;
        return Some(LaneDiagnosticFinding {
            tool: tool.to_string(),
            severity: "error".into(),
            file: Some(file.to_string()),
            line: line_number.trim_end_matches(':').parse().ok(),
            message: "rustfmt would change this file".into(),
            ..Default::default()
        });
    }
    if tool.contains("tsc") || tool.contains("tsgo") {
        return parse_typescript_finding(tool, trimmed);
    }
    if matches!(tool, "fmt" | "cargo-fmt" | "clippy" | "cargo-check" | "tsc" | "tsgo" | "tsc-build" | "bacon") {
        return None;
    }
    if tool == "machete" {
        let lower = trimmed.to_ascii_lowercase();
        if !lower.contains("unused") || lower.contains("no unused") || lower.contains("0 unused") {
            return None;
        }
    }
    if tool == "jscpd" {
        let lower = trimmed.to_ascii_lowercase();
        if (!lower.contains("clone") && !lower.contains("duplicate"))
            || lower.contains("0 clone")
            || lower.contains("0 duplicate")
            || lower.contains("clones: 0")
            || lower.contains("duplicates: 0")
            || lower.contains("no clone")
            || lower.contains("no duplicate")
        {
            return None;
        }
    }
    if tool.contains("gate") {
        let lower = trimmed.to_ascii_lowercase();
        let failure_words = ["error", "missing", "stale", "fail", "would", "unused", "duplicate", "refused"];
        if !failure_words.iter().any(|word| lower.contains(word)) {
            return None;
        }
    }
    if tool == "machete" || tool == "jscpd" || tool.contains("gate") {
        let (file, line, column) = diagnostic_text_location(trimmed);
        return Some(LaneDiagnosticFinding {
            tool: tool.to_string(),
            severity: if tool == "jscpd" { "warning".into() } else { "error".into() },
            file,
            line,
            column,
            message: trimmed.to_string(),
            ..Default::default()
        });
    }
    None
}

fn diagnostic_text_location(line: &str) -> (Option<String>, Option<u32>, Option<u32>) {
    for token in line.split_whitespace() {
        let token = token.trim_matches(|character: char| {
            matches!(character, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '`' | '\'')
        });
        let Some((prefix, last)) = token.rsplit_once(':') else {
            continue;
        };
        let Ok(last) = last.parse::<u32>() else {
            continue;
        };
        if let Some((file, line_number)) = prefix.rsplit_once(':') {
            if let Ok(line_number) = line_number.parse::<u32>() {
                if file.contains('/') || file.contains('\\') {
                    return (Some(file.to_string()), Some(line_number), Some(last));
                }
            }
        }
        if prefix.contains('/') || prefix.contains('\\') {
            return (Some(prefix.to_string()), Some(last), None);
        }
    }
    (None, None, None)
}

fn parse_typescript_finding(tool: &str, line: &str) -> Option<LaneDiagnosticFinding> {
    let (location, message) = line.split_once(": ")?;
    let (file, line_number, column) = if let Some((file, coords)) = location.rsplit_once('(') {
        let coords = coords.trim_end_matches(')');
        let (line_number, column) = coords.split_once(',')?;
        (file, line_number.parse().ok()?, column.parse().ok()?)
    } else {
        let mut parts = location.rsplitn(3, ':');
        let column = parts.next()?.parse().ok()?;
        let line_number = parts.next()?.parse().ok()?;
        (parts.next()?, line_number, column)
    };
    let (severity, message) = if let Some(message) = message.strip_prefix("error ") {
        ("error", message)
    } else if let Some(message) = message.strip_prefix("warning ") {
        ("warning", message)
    } else {
        return None;
    };
    let (code, message) = match message.split_once(':') {
        Some((code, message)) if code.trim().starts_with("TS") => {
            (Some(code.trim().to_string()), message.trim())
        }
        _ => (None, message.trim()),
    };
    Some(LaneDiagnosticFinding {
        tool: tool.to_string(),
        severity: severity.into(),
        file: Some(file.to_string()),
        line: Some(line_number),
        column: Some(column),
        message: message.to_string(),
        code,
    })
}

impl super::ServiceContext {
    pub(crate) fn diagnostics_request_for_report(
        &self,
        tab_id: &str,
        plan_id: &str,
        task_id: &str,
        details: &crate::agent::reports::ReportDetails,
    ) -> Option<DiagnosticsRequest> {
        let plan = self.plan_store.get(plan_id)?;
        let task = plan.tasks.iter().find(|task| task.id == task_id)?;
        let lane = task
            .lane
            .worktree_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())?;
        let marker = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.provisioned_cwd.clone())?;
        if !super::same_path(lane, &marker) {
            return None;
        }
        let attempt_id = details
            .attempt_id
            .clone()
            .or_else(|| {
                task.execution_attempts
                    .iter()
                    .rev()
                    .find(|attempt| {
                        attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
                    })
                    .map(|attempt| attempt.id.clone())
            })?;
        Some(DiagnosticsRequest {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id,
            canonical_project_path: canonical_project_path(&plan),
            lane_worktree_path: lane.to_string(),
            lane_marker: Some(marker),
            lane_branch: details.branch.clone().or_else(|| task.lane.branch.clone()),
            base: details.base.clone().or_else(|| task.lane.base.clone()),
            sha: details.sha.clone(),
            changed_files: details.changed_files.clone().unwrap_or_default(),
            require_lane_marker: true,
        })
    }

    pub(crate) fn check_lane_subject(
        &self,
        plan_ref: Option<&str>,
        subject: &str,
    ) -> Result<(String, String, LaneDiagnosticsSummary), String> {
        let (plan, task) = resolve_lane_subject(&self.plan_store, plan_ref, subject)?;
        let attempt_id = task
            .execution_attempts
            .iter()
            .rev()
            .find(|attempt| attempt.closed_at.is_none())
            .map(|attempt| attempt.id.clone())
            .unwrap_or_else(|| "manual".into());
        let lane = task
            .lane
            .worktree_path
            .clone()
            .ok_or_else(|| "task has no host-owned lane worktree".to_string())?;
        let request = DiagnosticsRequest {
            plan_id: plan.id.clone(),
            task_id: task.id.clone(),
            attempt_id,
            canonical_project_path: canonical_project_path(&plan),
            lane_worktree_path: lane,
            lane_marker: None,
            lane_branch: task.lane.branch.clone(),
            base: task.lane.base.clone(),
            sha: None,
            changed_files: Vec::new(),
            require_lane_marker: false,
        };
        let summary = self.lane_diagnostics.run(request)?;
        Ok((plan.id, task.id, summary))
    }

    pub(crate) fn diagnostics_for_task(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<LaneDiagnosticsSummary, String> {
        let plan = self
            .plan_store
            .get(plan_id)
            .ok_or_else(|| "plan not found".to_string())?;
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| "task not found".to_string())?;
        if let Some(summary) = task
            .execution_attempts
            .iter()
            .rev()
            .filter_map(|attempt| attempt.report.as_ref())
            .find_map(|report| report.diagnostics.clone())
        {
            return Ok(summary);
        }
        let attempt_id = task
            .execution_attempts
            .iter()
            .rev()
            .map(|attempt| attempt.id.as_str())
            .next()
            .ok_or_else(|| "task has no execution attempt".to_string())?;
        self.lane_diagnostics.load(plan_id, task_id, attempt_id)
    }
}

fn canonical_project_path(plan: &codeterm_types::plan::Plan) -> String {
    if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.clone()
    } else {
        plan.canonical_project_path.clone()
    }
}

fn resolve_lane_subject(
    store: &crate::service::plan::PlanStore,
    plan_ref: Option<&str>,
    subject: &str,
) -> Result<(codeterm_types::plan::Plan, codeterm_types::plan::Task), String> {
    if let Ok(resolved) = store.resolve_task_ref(plan_ref, subject) {
        if let Some(plan) = store.get(&resolved.plan_id) {
            if let Some(task) = plan.tasks.iter().find(|task| task.id == resolved.task_id) {
                if task.lane.worktree_path.is_some() {
                    return Ok((plan, task.clone()));
                }
            }
        }
    }
    let plans = match plan_ref {
        Some(reference) => vec![
            store
                .get(&store.resolve_plan_id(reference).map_err(|error| error.to_string())?)
                .ok_or_else(|| "plan not found".to_string())?,
        ],
        None => store.list_full(),
    };
    let mut matches = Vec::new();
    for plan in plans {
        for task in &plan.tasks {
            let branch_matches = task
                .lane
                .branch
                .as_deref()
                .is_some_and(|branch| branch == subject);
            let path_matches = task
                .lane
                .worktree_path
                .as_deref()
                .is_some_and(|path| path == subject);
            if branch_matches || path_matches {
                matches.push((plan.clone(), task.clone()));
            }
        }
    }
    match matches.as_slice() {
        [(plan, task)] => Ok((plan.clone(), task.clone())),
        [] => Err(format!("no lane task matched '{subject}'")),
        _ => Err(format!("lane subject '{subject}' is ambiguous; pass --plan")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_path_never_allows_a_watcher_but_canonical_does() {
        assert!(!analysis_allowed(analysis_scope(
            "C:/repo",
            "C:/repo-worktrees/.diagnostics",
            Some("C:/repo-worktrees/lane-a"),
            Some("C:/repo-worktrees/lane-a"),
            "C:/repo-worktrees/lane-a",
        )));
        assert!(analysis_allowed(analysis_scope(
            "C:/repo",
            "C:/repo-worktrees/.diagnostics",
            Some("C:/repo-worktrees/lane-a"),
            Some("C:/repo-worktrees/lane-a"),
            "C:/repo",
        )));
    }

    #[test]
    fn cargo_json_missing_import_keeps_primary_location_and_code() {
        let output = r#"CODETERM_DIAGNOSTIC_TOOL	cargo-check
{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `x`","code":{"code":"E0425"},"spans":[{"file_name":"src/main.rs","line_start":7,"column_start":9,"is_primary":true}]}}"#;
        let findings = parse_diagnostics(output, &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file.as_deref(), Some("src/main.rs"));
        assert_eq!(findings[0].line, Some(7));
        assert_eq!(findings[0].code.as_deref(), Some("E0425"));
    }

    #[test]
    fn diagnostics_filter_to_changed_source_files() {
        let output = concat!(
            "CODETERM_DIAGNOSTIC_TOOL\tcargo-check\n",
            r#"{"reason":"compiler-message","message":{"level":"warning","message":"changed","spans":[{"file_name":"src/changed.rs","line_start":2,"is_primary":true}]}}"#, "\n",
            r#"{"reason":"compiler-message","message":{"level":"warning","message":"unrelated","spans":[{"file_name":"src/other.rs","line_start":9,"is_primary":true}]}}"#,
        );
        let findings = parse_diagnostics(output, &["src-tauri/src/changed.rs".into()]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file.as_deref(), Some("src/changed.rs"));
    }

    #[test]
    fn clean_tool_output_does_not_create_findings() {
        let output = concat!(
            "CODETERM_DIAGNOSTIC_TOOL\tcargo-check\nChecking codeterm\n",
            "CODETERM_DIAGNOSTIC_EXIT\tcargo-check\t0\n",
            "CODETERM_DIAGNOSTIC_TOOL\twebforks-gate\nweb fork inventory is fresh\n",
            "CODETERM_DIAGNOSTIC_EXIT\twebforks-gate\t0\n",
        );
        assert!(parse_diagnostics(output, &[]).is_empty());
    }

    #[test]
    fn typescript_type_mismatch_keeps_file_line_column_and_code() {
        let findings = parse_diagnostics(
            "CODETERM_DIAGNOSTIC_TOOL\ttsc\nsrc/app.ts(12,4): error TS2322: type mismatch",
            &[],
        );
        assert_eq!(findings[0].file.as_deref(), Some("src/app.ts"));
        assert_eq!(findings[0].line, Some(12));
        assert_eq!(findings[0].column, Some(4));
        assert_eq!(findings[0].code.as_deref(), Some("TS2322"));
    }

    #[test]
    fn fmt_and_gate_findings_keep_their_tool_identity() {
        let findings = parse_diagnostics(
            "CODETERM_DIAGNOSTIC_TOOL\tfmt\nDiff in src/lib.rs at line 3:\nCODETERM_DIAGNOSTIC_TOOL\twebforks-gate\nmissing inventory entry",
            &["src/lib.rs".into()],
        );
        assert_eq!(findings[0].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[1].tool, "webforks-gate");
    }

    #[test]
    fn touched_crates_are_unique_and_diff_scoped() {
        let crates = touched_crates(&[
            "src-tauri/src/lib.rs".into(),
            "src-tauri/crates/codeterm-plan/src/lib.rs".into(),
            "src-tauri/crates/codeterm-plan/src/other.rs".into(),
            "cli/src/main.rs".into(),
        ]);
        assert_eq!(crates, vec!["app", "plan", "cli"]);
    }

    #[test]
    fn lane_diagnostics_summary_round_trips_counts_and_locations() {
        let summary = LaneDiagnosticsSummary::from_findings(
            "plan".into(),
            "task".into(),
            "attempt".into(),
            "warm".into(),
            vec!["cargo-check".into()],
            vec![
                LaneDiagnosticFinding {
                    tool: "cargo-check".into(),
                    severity: "error".into(),
                    file: Some("src/lib.rs".into()),
                    line: Some(4),
                    column: Some(2),
                    message: "missing import".into(),
                    code: Some("E0432".into()),
                },
                LaneDiagnosticFinding {
                    tool: "fmt".into(),
                    severity: "warning".into(),
                    ..Default::default()
                },
            ],
        );
        let json = serde_json::to_string(&summary).unwrap();
        let round_trip: LaneDiagnosticsSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.error_count, 1);
        assert_eq!(round_trip.warning_count, 1);
        assert_eq!(round_trip.findings[0].line, Some(4));
        assert_eq!(round_trip.findings[0].code.as_deref(), Some("E0432"));
    }

    #[test]
    fn incomplete_diagnostics_are_explicitly_bounded() {
        let summary = LaneDiagnosticsSummary::incomplete(
            "plan".into(),
            "task".into(),
            "attempt".into(),
            "warm".into(),
            "deadline exceeded".into(),
        );
        assert!(summary.incomplete);
        assert_eq!(summary.status, "incomplete");
        assert_eq!(summary.warning_count, 1);
    }
}
