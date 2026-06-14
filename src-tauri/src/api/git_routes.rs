use axum::{
    extract::{Path as AxumPath, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::api::routes::{check_auth, AppState};

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_PUSH_TIMEOUT: Duration = Duration::from_secs(60);
const OUTPUT_CAP: usize = 1024 * 1024;
const TRUNCATION_MARKER: &str = "\n<<TRUNCATED>>";
const FIELD_SEP: u8 = 0x1f;
const RECORD_SEP: u8 = 0x1e;

#[derive(Debug)]
pub(crate) struct GitOutput {
    pub stdout: Vec<u8>,
    pub status_ok: bool,
    pub stderr: String,
}

/// Run `git -C <cwd> --no-pager <args>` with a 10s timeout and a 1 MB output cap.
pub(crate) async fn run_git(cwd: &str, args: &[&str]) -> Result<GitOutput, String> {
    run_git_with_timeout(cwd, args, GIT_TIMEOUT).await
}

async fn run_git_with_timeout(
    cwd: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<GitOutput, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C").arg(cwd).arg("--no-pager").args(args);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    crate::process_util::no_window_tokio(&mut cmd);

    let child = cmd.spawn().map_err(|e| format!("spawn git: {}", e))?;
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(format!("wait git: {}", e)),
        Err(_) => return Err("git command timed out".to_string()),
    };

    let mut stdout = output.stdout;
    if stdout.len() > OUTPUT_CAP {
        stdout.truncate(OUTPUT_CAP);
    }
    Ok(GitOutput {
        stdout,
        status_ok: output.status.success(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

/// 404 unless `cwd` is an existing dir inside a git work tree.
async fn ensure_repo(cwd: &str) -> Result<(), (StatusCode, String)> {
    if cwd.is_empty() || !Path::new(cwd).is_dir() {
        return Err((StatusCode::NOT_FOUND, "not a directory".to_string()));
    }
    let out = run_git(cwd, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::NOT_FOUND, "not a git repository".to_string()));
    }
    Ok(())
}

fn validate_git_path(path: &str) -> Result<(), (StatusCode, String)> {
    if path.is_empty() || path.contains("..") || path.starts_with('-') {
        return Err((StatusCode::BAD_REQUEST, "invalid path".to_string()));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct GraphQuery {
    cwd: String,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    skip: u32,
}

fn default_limit() -> u32 {
    300
}

#[derive(Serialize)]
pub(crate) struct RefEntry {
    name: String,
    kind: String,
}

#[derive(Serialize)]
pub(crate) struct GraphCommit {
    sha: String,
    parents: Vec<String>,
    author: String,
    date: String,
    refs: Vec<RefEntry>,
    subject: String,
}

#[derive(Serialize)]
pub(crate) struct GraphResponse {
    commits: Vec<GraphCommit>,
    head_sha: Option<String>,
    branch: Option<String>,
    dirty: bool,
}

/// Classify a single decoration token from `%D` (`--decorate=full`).
fn classify_ref(token: &str) -> Option<RefEntry> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    // `%D` prefixes tag decorations with `tag: `.
    let token = token.strip_prefix("tag: ").unwrap_or(token);
    if let Some(rest) = token.strip_prefix("HEAD -> ") {
        // `HEAD -> refs/heads/main`: surface the branch it points at as a head ref.
        let name = rest.trim_start_matches("refs/heads/");
        return Some(RefEntry {
            name: name.to_string(),
            kind: "head".to_string(),
        });
    }
    if token == "HEAD" {
        return Some(RefEntry {
            name: "HEAD".to_string(),
            kind: "head".to_string(),
        });
    }
    if let Some(name) = token.strip_prefix("refs/heads/") {
        return Some(RefEntry {
            name: name.to_string(),
            kind: "branch".to_string(),
        });
    }
    if let Some(name) = token.strip_prefix("refs/remotes/") {
        return Some(RefEntry {
            name: name.to_string(),
            kind: "remote".to_string(),
        });
    }
    if let Some(name) = token.strip_prefix("refs/tags/") {
        let name = name.trim_end_matches("^{}");
        return Some(RefEntry {
            name: name.to_string(),
            kind: "tag".to_string(),
        });
    }
    None
}

fn parse_refs(field: &str) -> Vec<RefEntry> {
    field.split(',').filter_map(classify_ref).collect()
}

/// Parse the NUL-style `\x1f`-delimited, `\x1e`-terminated log stream.
fn parse_graph(stdout: &[u8]) -> Vec<GraphCommit> {
    let mut commits = Vec::new();
    for record in stdout.split(|&b| b == RECORD_SEP) {
        if record.is_empty() {
            continue;
        }
        // Records after the first carry a leading '\n' from the separator output.
        let record = if record.first() == Some(&b'\n') {
            &record[1..]
        } else {
            record
        };
        if record.is_empty() {
            continue;
        }
        let fields: Vec<&[u8]> = record.split(|&b| b == FIELD_SEP).collect();
        // Format emits a leading FIELD_SEP, so fields[0] is empty; real data starts at 1.
        if fields.len() < 7 {
            continue;
        }
        let s = |i: usize| String::from_utf8_lossy(fields[i]).to_string();
        let sha = s(1);
        if sha.is_empty() {
            continue;
        }
        let parents: Vec<String> = s(2).split_whitespace().map(|p| p.to_string()).collect();
        commits.push(GraphCommit {
            sha,
            parents,
            author: s(3),
            date: s(4),
            refs: parse_refs(&s(5)),
            subject: s(6),
        });
    }
    commits
}

pub async fn git_graph(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<GraphQuery>,
) -> Result<Json<GraphResponse>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&q.cwd).await?;

    let limit = q.limit.to_string();
    let skip = q.skip.to_string();
    let pretty = "--pretty=format:%x1f%H%x1f%P%x1f%an%x1f%aI%x1f%D%x1f%s%x1e";
    let log = run_git(
        &q.cwd,
        &[
            "-c",
            "core.quotePath=false",
            "log",
            "--branches",
            "--tags",
            "--remotes",
            "HEAD",
            "--topo-order",
            "--decorate=full",
            pretty,
            "-n",
            &limit,
            "--skip",
            &skip,
        ],
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !log.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, log.stderr));
    }
    let commits = parse_graph(&log.stdout);

    let head_sha = run_git(&q.cwd, &["rev-parse", "HEAD"])
        .await
        .ok()
        .filter(|o| o.status_ok)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let branch = run_git(&q.cwd, &["symbolic-ref", "--short", "-q", "HEAD"])
        .await
        .ok()
        .filter(|o| o.status_ok)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let dirty = run_git(&q.cwd, &["status", "--porcelain"])
        .await
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    Ok(Json(GraphResponse {
        commits,
        head_sha,
        branch,
        dirty,
    }))
}

#[derive(Deserialize)]
pub(crate) struct CwdQuery {
    cwd: String,
}

#[derive(Serialize, Debug)]
pub(crate) struct FileChange {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<String>,
    added: Option<u64>,
    deleted: Option<u64>,
    status: String,
}

/// One numstat line: `<added>\t<deleted>\t<path>` where path may encode a rename.
fn parse_numstat_line(line: &str) -> Option<FileChange> {
    let mut parts = line.splitn(3, '\t');
    let added_raw = parts.next()?;
    let deleted_raw = parts.next()?;
    let path_raw = parts.next()?;
    let added = if added_raw == "-" {
        None
    } else {
        added_raw.parse::<u64>().ok()
    };
    let deleted = if deleted_raw == "-" {
        None
    } else {
        deleted_raw.parse::<u64>().ok()
    };
    let binary = added_raw == "-" && deleted_raw == "-";

    // Rename forms: `old => new` or `dir/{old => new}/file`.
    if let Some((old_path, path)) = split_rename(path_raw) {
        return Some(FileChange {
            path,
            old_path: Some(old_path),
            added,
            deleted,
            status: "renamed".to_string(),
        });
    }
    let status = if binary { "binary" } else { "modified" };
    Some(FileChange {
        path: path_raw.to_string(),
        old_path: None,
        added,
        deleted,
        status: status.to_string(),
    })
}

/// Resolve git's `old => new` rename notation, including the `{a => b}` brace form.
fn split_rename(path: &str) -> Option<(String, String)> {
    if !path.contains("=>") {
        return None;
    }
    if let Some(open) = path.find('{') {
        if let Some(close) = path[open..].find('}').map(|i| i + open) {
            let prefix = &path[..open];
            let inner = &path[open + 1..close];
            let suffix = &path[close + 1..];
            let (a, b) = inner.split_once("=>")?;
            let old = format!("{}{}{}", prefix, a.trim(), suffix);
            let new = format!("{}{}{}", prefix, b.trim(), suffix);
            return Some((normalize_path(&old), normalize_path(&new)));
        }
    }
    let (a, b) = path.split_once("=>")?;
    Some((a.trim().to_string(), b.trim().to_string()))
}

fn normalize_path(p: &str) -> String {
    p.replace("//", "/")
}

fn parse_numstat(stdout: &[u8]) -> Vec<FileChange> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(parse_numstat_line)
        .collect()
}

pub async fn git_commit_files(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    AxumPath(sha): AxumPath<String>,
    Query(q): Query<CwdQuery>,
) -> Result<Json<Vec<FileChange>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&q.cwd).await?;
    if !is_valid_sha(&sha) {
        return Err((StatusCode::BAD_REQUEST, "invalid sha".to_string()));
    }

    let out = run_git(
        &q.cwd,
        &["show", "-M", "-C", "--numstat", "--format=", &sha],
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::NOT_FOUND, out.stderr));
    }
    Ok(Json(parse_numstat(&out.stdout)))
}

#[derive(Deserialize)]
pub(crate) struct DiffQuery {
    cwd: String,
    path: String,
}

/// A merge commit has more than one parent (space-separated SHAs from `%P`).
async fn is_merge(cwd: &str, sha: &str) -> bool {
    run_git(cwd, &["rev-list", "--parents", "-n", "1", sha])
        .await
        .ok()
        .filter(|o| o.status_ok)
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .count()
                > 2
        })
        .unwrap_or(false)
}

pub async fn git_commit_diff(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    AxumPath(sha): AxumPath<String>,
    Query(q): Query<DiffQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&q.cwd).await?;
    if !is_valid_sha(&sha) {
        return Err((StatusCode::BAD_REQUEST, "invalid sha".to_string()));
    }

    let merge = is_merge(&q.cwd, &sha).await;
    let parent_spec = format!("{}^1", sha);
    let out = if merge {
        // Merges diff against the first parent only (no combined diff in v1).
        run_git(
            &q.cwd,
            &[
                "diff",
                "-M",
                "-C",
                "--no-color",
                &parent_spec,
                &sha,
                "--",
                &q.path,
            ],
        )
        .await
    } else {
        run_git(
            &q.cwd,
            &[
                "show",
                "-M",
                "-C",
                "--no-color",
                "--format=",
                &sha,
                "--",
                &q.path,
            ],
        )
        .await
    }
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::NOT_FOUND, out.stderr));
    }

    let text = render_capped(&out.stdout);
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        text,
    ))
}

#[derive(Deserialize)]
pub(crate) struct WorkdirQuery {
    cwd: String,
    path: String,
}

pub async fn git_workdir_diff(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkdirQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&q.cwd).await?;

    let out = run_git(
        &q.cwd,
        &["diff", "-M", "-C", "--no-color", "HEAD", "--", &q.path],
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::NOT_FOUND, out.stderr));
    }
    let text = render_capped(&out.stdout);
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        text,
    ))
}

#[derive(Serialize, Debug, PartialEq)]
pub(crate) struct WorkdirFile {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<String>,
    status: String,
    staged: bool,
    added: Option<u64>,
    deleted: Option<u64>,
}

/// Map a porcelain XY status pair to our coarse status vocabulary.
fn classify_porcelain(xy: &[u8]) -> &'static str {
    if xy.len() < 2 {
        return "modified";
    }
    let x = xy[0];
    let y = xy[1];
    if x == b'?' && y == b'?' {
        return "untracked";
    }
    // Conflict states per `git status` docs: any side U, or DD / AA.
    if x == b'U' || y == b'U' || (x == b'D' && y == b'D') || (x == b'A' && y == b'A') {
        return "conflicted";
    }
    if x == b'R' || y == b'R' {
        return "renamed";
    }
    if x == b'D' || y == b'D' {
        return "deleted";
    }
    // Index side set (and not unmodified/untracked) → staged.
    if x != b' ' && x != b'?' {
        return "staged";
    }
    "modified"
}

/// Parse `git status --porcelain=v1 -z`: NUL-separated entries; a rename entry
/// ("R" in XY) is immediately followed by a second NUL-separated field holding
/// the old path. Returns `(path, old_path, status)` tuples.
type ParsedStatus = (String, Option<String>, &'static str, bool);

fn parse_status_porcelain(stdout: &[u8]) -> Vec<ParsedStatus> {
    let mut out = Vec::new();
    let mut fields = stdout.split(|&b| b == 0).filter(|f| !f.is_empty());
    while let Some(entry) = fields.next() {
        if entry.len() < 3 {
            continue;
        }
        let xy = &entry[0..2];
        let staged = xy[0] != b' ' && xy[0] != b'?';
        let path = String::from_utf8_lossy(&entry[3..]).to_string();
        let status = classify_porcelain(xy);
        if status == "renamed" {
            // The old path is the next NUL-separated field in -z output.
            let old_path = fields
                .next()
                .map(|f| String::from_utf8_lossy(f).to_string());
            out.push((path, old_path, status, staged));
        } else {
            out.push((path, None, status, staged));
        }
    }
    out
}

/// Index `git diff --numstat HEAD` by path → (added, deleted), `-` → None (binary).
fn parse_numstat_counts(
    stdout: &[u8],
) -> std::collections::HashMap<String, (Option<u64>, Option<u64>)> {
    let mut map = std::collections::HashMap::new();
    for line in String::from_utf8_lossy(stdout).lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(a), Some(d), Some(p)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let added = if a == "-" { None } else { a.parse().ok() };
        let deleted = if d == "-" { None } else { d.parse().ok() };
        // numstat encodes renames as `old => new`; key on the resolved new path.
        let path = split_rename(p)
            .map(|(_, new)| new)
            .unwrap_or_else(|| p.to_string());
        map.insert(path, (added, deleted));
    }
    map
}

fn merge_workdir_files(
    statuses: Vec<ParsedStatus>,
    counts: &std::collections::HashMap<String, (Option<u64>, Option<u64>)>,
) -> Vec<WorkdirFile> {
    statuses
        .into_iter()
        .map(|(path, old_path, status, staged)| {
            // Untracked files have no numstat-vs-HEAD counts.
            let (added, deleted) = if status == "untracked" {
                (None, None)
            } else {
                counts.get(&path).copied().unwrap_or((None, None))
            };
            WorkdirFile {
                path,
                old_path,
                status: status.to_string(),
                staged,
                added,
                deleted,
            }
        })
        .collect()
}

pub async fn git_workdir_files(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<CwdQuery>,
) -> Result<Json<Vec<WorkdirFile>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&q.cwd).await?;

    let status = run_git(
        &q.cwd,
        &[
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain=v1",
            "-z",
        ],
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !status.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, status.stderr));
    }

    let numstat = run_git(
        &q.cwd,
        &["-c", "core.quotePath=false", "diff", "--numstat", "HEAD"],
    )
    .await
    .map(|o| parse_numstat_counts(&o.stdout))
    .unwrap_or_default();

    let files = merge_workdir_files(parse_status_porcelain(&status.stdout), &numstat);
    Ok(Json(files))
}

#[derive(Deserialize)]
pub(crate) struct RevertFileBody {
    cwd: String,
    path: String,
}

/// Surgical mutating endpoint: arg-vector only, `--` guards the path.
pub async fn git_revert_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RevertFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;

    validate_git_path(&body.path)?;

    // Look up the file's porcelain status before mutating.
    let status = run_git(
        &body.cwd,
        &[
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain=v1",
            "-z",
            "--",
            &body.path,
        ],
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !status.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, status.stderr));
    }
    let entry_status = parse_status_porcelain(&status.stdout)
        .into_iter()
        .find(|(p, _, _, _)| *p == body.path)
        .map(|(_, _, s, _)| s);
    match entry_status {
        Some("untracked") => {
            return Err((
                StatusCode::CONFLICT,
                "untracked file — delete it manually".to_string(),
            ))
        }
        Some("conflicted") => {
            return Err((
                StatusCode::CONFLICT,
                "conflicted file — resolve it manually".to_string(),
            ))
        }
        _ => {}
    }

    let out = run_git(&body.cwd, &["checkout", "HEAD", "--", &body.path])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, out.stderr));
    }
    Ok(Json(serde_json::json!({ "reverted": true })))
}

#[derive(Deserialize)]
pub(crate) struct GitFileBody {
    cwd: String,
    path: String,
}

pub async fn git_stage_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<GitFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;
    validate_git_path(&body.path)?;

    let out = run_git(&body.cwd, &["add", "--", &body.path])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, out.stderr));
    }
    Ok(Json(serde_json::json!({ "staged": true })))
}

pub async fn git_unstage_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<GitFileBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;
    validate_git_path(&body.path)?;

    let out = run_git(&body.cwd, &["restore", "--staged", "--", &body.path])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, out.stderr));
    }
    Ok(Json(serde_json::json!({ "unstaged": true })))
}

#[derive(Deserialize)]
pub(crate) struct GitFilesBody {
    cwd: String,
    paths: Vec<String>,
}

fn validate_git_paths(paths: &[String]) -> Result<(), (StatusCode, String)> {
    if paths.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no paths".to_string()));
    }
    for path in paths {
        validate_git_path(path)?;
    }
    Ok(())
}

pub async fn git_stage_files(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<GitFilesBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;
    validate_git_paths(&body.paths)?;

    let mut args = vec!["add", "--"];
    args.extend(body.paths.iter().map(String::as_str));
    let out = run_git(&body.cwd, &args)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, out.stderr));
    }
    Ok(Json(serde_json::json!({ "staged": true })))
}

pub async fn git_unstage_files(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<GitFilesBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;
    validate_git_paths(&body.paths)?;

    let mut args = vec!["restore", "--staged", "--"];
    args.extend(body.paths.iter().map(String::as_str));
    let out = run_git(&body.cwd, &args)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, out.stderr));
    }
    Ok(Json(serde_json::json!({ "unstaged": true })))
}

#[derive(Deserialize)]
pub(crate) struct CommitBody {
    cwd: String,
    message: String,
}

pub async fn git_commit(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CommitBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;

    let message = body.message.trim();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "commit message is empty".to_string()));
    }

    let out = run_git(&body.cwd, &["commit", "-m", message])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::CONFLICT, out.stderr));
    }
    Ok(Json(serde_json::json!({
        "committed": true,
        "stdout": render_capped(&out.stdout),
    })))
}

#[derive(Deserialize)]
pub(crate) struct PushBody {
    cwd: String,
}

pub async fn git_push(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PushBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_repo(&body.cwd).await?;

    let out = run_git_with_timeout(&body.cwd, &["push"], GIT_PUSH_TIMEOUT)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !out.status_ok {
        return Err((StatusCode::CONFLICT, out.stderr));
    }
    Ok(Json(serde_json::json!({
        "pushed": true,
        "stdout": render_capped(&out.stdout),
        "stderr": out.stderr,
    })))
}

#[derive(Deserialize)]
pub(crate) struct ReposQuery {
    cwd: String,
}

#[derive(Serialize)]
pub(crate) struct RepoEntry {
    path: String,
    name: String,
    branch: Option<String>,
}

const REPO_SCAN_CAP: usize = 20;

/// Discover repos at `cwd`: the work tree itself, or depth-1 child repos when
/// `cwd` is a plain folder holding several checkouts.
pub async fn git_repos(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ReposQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    if q.cwd.is_empty() || !Path::new(&q.cwd).is_dir() {
        return Err((StatusCode::NOT_FOUND, "not a directory".to_string()));
    }

    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    let top = run_git(&q.cwd, &["rev-parse", "--show-toplevel"])
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if top.status_ok {
        let root = String::from_utf8_lossy(&top.stdout).trim().to_string();
        roots.push(root.into());
    } else if let Ok(entries) = std::fs::read_dir(&q.cwd) {
        let mut children: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.join(".git").exists())
            .collect();
        children.sort();
        children.truncate(REPO_SCAN_CAP);
        roots = children;
    }

    let mut repos = Vec::with_capacity(roots.len());
    for root in roots {
        let cwd = root.to_string_lossy().to_string();
        let branch = match run_git(&cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
            Ok(o) if o.status_ok => Some(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            _ => None,
        };
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cwd.clone());
        repos.push(RepoEntry {
            path: cwd,
            name,
            branch,
        });
    }
    Ok(Json(serde_json::json!({ "repos": repos })))
}

/// Lossy-decode the patch, re-appending the truncation marker when the byte cap clipped it.
fn render_capped(stdout: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).to_string();
    if stdout.len() >= OUTPUT_CAP {
        text.push_str(TRUNCATION_MARKER);
    }
    text
}

/// Accept hex SHAs and the `^N`/`~N` suffix; reject anything else so the value never reaches git as an option.
fn is_valid_sha(sha: &str) -> bool {
    !sha.is_empty()
        && sha.len() <= 64
        && sha
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '^' || c == '~')
}

#[cfg(test)]
mod tests;
