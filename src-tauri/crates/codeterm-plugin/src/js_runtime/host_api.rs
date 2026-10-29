//! `host` global injected into every plugin VM — the generic FS / sqlite /
//! shell-quoting / env / logging primitives that let all per-provider logic
//! live in TS.
//!
//! Contract: all functions are synchronous (QuickJS blocks their VM, so nothing
//! here may block on the hot path); complex data is returned as a JSON string;
//! errors are signalled by `null`; a breached size bound throws a RangeError.
//!
//! Safety: bundled/in-workspace plugins get the full surface; a drop-in under
//! `~/.domios/plugins/` is confined by `PluginSandbox` (FS/sqlite scoped to
//! its own dir + declared session root, secret-looking env vars blocked).
//! `dataDir` discloses only the instance path; FS/sqlite/env grants are unchanged.

/// Version of the plugin/host contract; a drop-in must declare a matching `hostApi`
/// or it's skipped at load. Bump on any breaking change to the host or `AgentProvider` surface.
pub const HOST_API_VERSION: u32 = 2;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

mod filesystem;
pub(super) mod sqlite;

use super::manifest::CredentialConfig;
use crate::host_traits::PluginHostCtx;
use codeterm_decision::TokenMode;

const PLUGIN_READ_FILE_LIMIT: u64 = 1024 * 1024;

struct HeldFileLock {
    call_id: String,
    path: std::path::PathBuf,
    file: File,
}

impl Drop for HeldFileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn held_file_locks() -> &'static parking_lot::Mutex<HashMap<String, HeldFileLock>> {
    static LOCKS: OnceLock<parking_lot::Mutex<HashMap<String, HeldFileLock>>> = OnceLock::new();
    LOCKS.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

pub(super) fn release_bundle_file_locks(directory: &Path) {
    if let Ok(directory) = directory.canonicalize() {
        held_file_locks()
            .lock()
            .retain(|_, lock| !lock.path.starts_with(&directory));
    }
}

thread_local! {
    static FILE_LOCK_CALL: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) struct FileLockCallScope {
    call_id: String,
    previous: Option<String>,
}

impl FileLockCallScope {
    pub(super) fn new() -> Self {
        let call_id = uuid::Uuid::new_v4().to_string();
        let previous = FILE_LOCK_CALL.with(|scope| scope.replace(Some(call_id.clone())));
        Self { call_id, previous }
    }
}

impl Drop for FileLockCallScope {
    fn drop(&mut self) {
        release_call_file_locks(&self.call_id);
        FILE_LOCK_CALL.with(|scope| *scope.borrow_mut() = self.previous.take());
    }
}

fn current_file_lock_call() -> Option<String> {
    FILE_LOCK_CALL.with(|scope| scope.borrow().clone())
}

fn release_call_file_locks(call_id: &str) {
    held_file_locks()
        .lock()
        .retain(|_, lock| lock.call_id != call_id);
}

/// A write-once, weakly-held ref populated after construction — the VM is built
/// before the `ServiceContext` exists, so it can't be a constructor argument.
/// `Weak` so a sandbox outliving teardown can't pin the service alive.
pub(super) struct LateRef<T: ?Sized>(OnceLock<Weak<T>>);

impl<T: ?Sized> LateRef<T> {
    pub(super) fn new() -> Self {
        Self(OnceLock::new())
    }

    /// Populate the ref. First write wins (OnceLock); returns whether this call
    /// set it (`false` if it was already populated).
    pub(super) fn set(&self, weak: Weak<T>) -> bool {
        self.0.set(weak).is_ok()
    }

    /// Resolve to a live `Arc`, or `None` before population / after the target is dropped.
    pub(super) fn get(&self) -> Option<Arc<T>> {
        self.0.get().and_then(Weak::upgrade)
    }
}

/// FS/env policy for a plugin's `host.*` calls. Bundled/workspace plugins are
/// unrestricted; a user drop-in is confined to `read_roots`/`write_roots` with
/// secret-looking `envGet` names blocked, so it can't slurp `~/.ssh` or the API token.
pub(super) struct PluginSandbox {
    unrestricted: bool,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    project_read_roots: Vec<PathBuf>,
    project_write_roots: Vec<PathBuf>,
    denied_writes: Vec<PathBuf>,
    /// Read-roots granted at runtime for tab cwds the HOST hands the plugin
    /// (bubble/glance ctx). Never fed from plugin-authored values (view args).
    granted_read_roots: parking_lot::Mutex<Vec<PathBuf>>,
    /// Files inside a read root the plugin still may not read — every declared
    /// credential file. Declaring a credential is what revokes direct access.
    denied_reads: Vec<PathBuf>,
    /// Credential files the host resolves on the plugin's behalf.
    credentials: Vec<CredentialConfig>,
    /// Monotonic count of sandbox denials (`can_read`/`can_env`). A caller
    /// snapshots it around a plugin call to tell "this plugin lost a capability
    /// it needs" apart from "the source itself failed".
    denials: std::sync::atomic::AtomicU64,
    /// Plugin id — namespaces the per-plugin secret bucket, so `host.secret*` is isolated.
    plugin_id: String,
    /// `host.exec` binary allowlist (host_api v2). Empty unless declared.
    exec_allow: Vec<String>,
    /// `host.fetch` host allowlist (host_api v2). Empty unless declared.
    net_allow: Vec<String>,
    token_mode: TokenMode,
    /// Whether `host.secret*` is permitted.
    secrets_allowed: bool,
    /// `agent_commands` grant — gates routing a spawned agent's `domios plugin <id> <verb>` to `onAgentCommand`.
    agent_commands_allowed: bool,
    /// `surfaces` grant — gates `host.surface.call` driving the calling agent's authorized surface.
    surfaces_allowed: bool,
    /// Whether `host.workspace*` is permitted (`manage_workspaces` grant).
    manage_workspaces_allowed: bool,
    /// Whether `host.agent*` is permitted (`spawn_agents` grant).
    spawn_agents_allowed: bool,
    /// Whether `host.analytics` is permitted (`analytics` grant).
    analytics_allowed: bool,
    graph_events_allowed: bool,
    local_recording_allowed: bool,
    receiver_status_query: Option<codeterm_local_recording::ReceiverStatusQuery>,
    /// The plugin's settings blob, returned by `host.settingsJson()`. Interior-mutable
    /// so a config-only hot-reload re-stamps it in place without a VM rebuild (which would wipe in-memory sessions).
    settings_json: parking_lot::Mutex<String>,
    /// Late-bound handle to the host `ServiceContext` (the VM is built before it exists).
    svc: LateRef<dyn PluginHostCtx>,
}

impl PluginSandbox {
    /// Full host access — bundled/workspace plugins and tests.
    pub(super) fn unrestricted() -> Self {
        Self {
            unrestricted: true,
            read_roots: Vec::new(),
            write_roots: Vec::new(),
            project_read_roots: Vec::new(),
            project_write_roots: Vec::new(),
            denied_writes: Vec::new(),
            granted_read_roots: parking_lot::Mutex::new(Vec::new()),
            denied_reads: Vec::new(),
            credentials: Vec::new(),
            denials: std::sync::atomic::AtomicU64::new(0),
            plugin_id: String::new(),
            exec_allow: Vec::new(),
            net_allow: Vec::new(),
            token_mode: TokenMode::None,
            secrets_allowed: true,
            agent_commands_allowed: true,
            surfaces_allowed: true,
            manage_workspaces_allowed: true,
            spawn_agents_allowed: true,
            analytics_allowed: true,
            graph_events_allowed: true,
            local_recording_allowed: false,
            receiver_status_query: None,
            settings_json: parking_lot::Mutex::new("null".to_string()),
            svc: LateRef::new(),
        }
    }

    pub(super) fn restricted(read_roots: Vec<PathBuf>, write_roots: Vec<PathBuf>) -> Self {
        Self {
            unrestricted: false,
            read_roots,
            write_roots,
            project_read_roots: Vec::new(),
            project_write_roots: Vec::new(),
            denied_writes: Vec::new(),
            granted_read_roots: parking_lot::Mutex::new(Vec::new()),
            denied_reads: Vec::new(),
            credentials: Vec::new(),
            denials: std::sync::atomic::AtomicU64::new(0),
            plugin_id: String::new(),
            exec_allow: Vec::new(),
            net_allow: Vec::new(),
            token_mode: TokenMode::None,
            secrets_allowed: false,
            agent_commands_allowed: false,
            surfaces_allowed: false,
            manage_workspaces_allowed: false,
            spawn_agents_allowed: false,
            analytics_allowed: false,
            graph_events_allowed: false,
            local_recording_allowed: false,
            receiver_status_query: None,
            settings_json: parking_lot::Mutex::new("null".to_string()),
            svc: LateRef::new(),
        }
    }

    /// Stamp the plugin identity + its effective permissions (manifest request
    /// merged with the user's persisted grants) onto a base sandbox.
    pub(super) fn with_identity_and_perms(
        mut self,
        plugin_id: &str,
        perms: &super::manifest::PermissionsConfig,
    ) -> Self {
        self.plugin_id = plugin_id.to_string();
        self.exec_allow = perms.subprocess.allow.clone();
        self.net_allow = perms.network.allow.clone();
        self.secrets_allowed = self.unrestricted || perms.secrets;
        self.agent_commands_allowed = self.unrestricted || perms.agent_commands;
        self.surfaces_allowed = self.unrestricted || perms.surfaces;
        self.manage_workspaces_allowed = self.unrestricted || perms.manage_workspaces;
        self.spawn_agents_allowed = self.unrestricted || perms.spawn_agents;
        self.analytics_allowed = self.unrestricted || perms.analytics;
        self.graph_events_allowed = self.unrestricted || perms.graph_events;
        self.local_recording_allowed = perms.local_recording;
        self
    }

    /// Stamp the plugin identity without touching permissions. The VM and its
    /// sandbox must agree on this: `host.exec`/`host.fetch` jobs are keyed by
    /// the sandbox's id, and the VM awaits and reaps them by the same one.
    pub(super) fn with_identity(mut self, plugin_id: &str) -> Self {
        self.plugin_id = plugin_id.to_string();
        self
    }

    /// Declare the manifest's credential files: the host resolves them, and the
    /// plugin loses `readFile` on those exact paths even inside a read root.
    pub(super) fn with_credentials(mut self, credentials: &[CredentialConfig]) -> Self {
        self.denied_reads = super::host_credentials::declared_files(credentials);
        self.credentials = credentials.to_vec();
        self
    }

    pub(super) fn with_token_mode(mut self, token_mode: TokenMode) -> Self {
        self.token_mode = token_mode;
        self
    }

    pub(super) fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// How many sandbox capabilities this plugin has been denied so far. Read
    /// before and after a plugin call: a rise means the call failed because the
    /// plugin is confined, not because its data source is down.
    pub(super) fn denial_count(&self) -> u64 {
        self.denials.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn record_denial(&self) {
        self.denials
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn credentials(&self) -> &[CredentialConfig] {
        &self.credentials
    }

    /// May the plugin run `bin` via `host.exec`?
    /// Accepts both bare names ("ffmpeg") and full paths ("/opt/homebrew/bin/ffmpeg")
    /// as long as the basename is in the allowlist.
    pub(super) fn can_exec(&self, bin: &str) -> bool {
        if !codeterm_decision::physical::effects_allowed() {
            return false;
        }
        if self.unrestricted {
            return true;
        }
        let basename = std::path::Path::new(bin)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(bin);
        self.exec_allow.iter().any(|b| b == bin || b == basename)
    }

    /// May the plugin reach `host` (host or `host:port`) via `host.fetch`?
    pub(super) fn can_reach(&self, host: &str) -> bool {
        if !codeterm_decision::physical::effects_allowed() {
            return false;
        }
        if self.unrestricted {
            return true;
        }
        let host = host.trim();
        codeterm_util::network_host::network_host_allowed(host, &self.net_allow)
    }

    pub(super) fn supporter_token_allowed(&self, host: &str) -> bool {
        codeterm_util::network_host::network_host_allowed(host, &self.net_allow)
    }

    pub(super) fn secrets_allowed(&self) -> bool {
        self.secrets_allowed
    }

    /// Whether the plugin holds the `agent_commands` grant (inverse-seam routing).
    pub(super) fn agent_commands_allowed(&self) -> bool {
        self.agent_commands_allowed
    }

    pub(super) fn surfaces_allowed(&self) -> bool {
        self.surfaces_allowed
    }

    /// Whether the plugin may create/manage workspaces (`host.workspace*`).
    pub(super) fn manage_workspaces_allowed(&self) -> bool {
        self.manage_workspaces_allowed
    }

    /// Whether the plugin may spawn/drive agents (`host.agent*`).
    pub(super) fn spawn_agents_allowed(&self) -> bool {
        self.spawn_agents_allowed
    }

    /// Whether the plugin may read machine-wide KPI aggregates (`host.analytics`).
    pub(super) fn analytics_allowed(&self) -> bool {
        self.analytics_allowed
    }

    pub(super) fn lifecycle_permission_allowed(&self, permission: &str) -> bool {
        if !codeterm_decision::physical::effects_allowed() {
            return false;
        }
        if let Some(host) = permission.strip_prefix("network:") {
            return self.can_reach(host);
        }
        match permission {
            "local_recording" => self.local_recording_allowed,
            "project_write" => !self.project_write_roots.is_empty(),
            "manage_workspaces" => self.manage_workspaces_allowed,
            "spawn_agents" => self.spawn_agents_allowed,
            "agent_commands" => self.agent_commands_allowed,
            "secrets" => self.secrets_allowed,
            "graph_events" => self.graph_events_allowed,
            _ => false,
        }
    }

    pub(super) fn graph_events_allowed(&self) -> bool {
        self.graph_events_allowed
    }

    pub(super) fn with_receiver_status_query(
        mut self,
        query: Option<codeterm_local_recording::ReceiverStatusQuery>,
    ) -> Self {
        self.receiver_status_query = query.filter(|query| query.valid());
        self
    }

    pub(super) fn receiver_status_query(
        &self,
    ) -> Option<&codeterm_local_recording::ReceiverStatusQuery> {
        self.receiver_status_query.as_ref()
    }

    pub(super) fn local_recording_allowed(&self) -> bool {
        codeterm_decision::physical::effects_allowed() && self.local_recording_allowed
    }

    /// Attach the plugin's settings blob (`config.yaml` → JSON) at load.
    pub(super) fn with_settings(self, settings_json: String) -> Self {
        *self.settings_json.lock() = settings_json;
        self
    }

    /// Re-stamp the settings blob in place on a config-only hot-reload, so a live VM picks it up without a rebuild.
    pub(super) fn set_settings(&self, settings_json: String) {
        *self.settings_json.lock() = settings_json;
    }

    pub(super) fn settings_json(&self) -> String {
        self.settings_json.lock().clone()
    }

    /// Late-bind the host `ServiceContext` (see [`LateRef`]). First write wins.
    pub(super) fn set_service(&self, svc: Weak<dyn PluginHostCtx>) -> bool {
        self.svc.set(svc)
    }

    /// Resolve the host `ServiceContext`, or `None` if not yet populated / gone.
    pub(super) fn service(&self) -> Option<Arc<dyn PluginHostCtx>> {
        if !codeterm_decision::physical::effects_allowed() {
            return None;
        }
        self.svc.get()
    }

    /// Oldest grant is evicted past this; a tab rarely visits this many cwds.
    const MAX_GRANTED_CWD_ROOTS: usize = 32;

    /// Record a host-supplied tab cwd so reads under it pass `can_read`.
    /// Callers must only pass paths the host itself assembled (bubble/glance
    /// ctx) — never plugin-authored values, or the sandbox self-widens.
    pub(super) fn grant_cwd_read_root(&self, cwd: &str) {
        if self.unrestricted || !Path::new(cwd).is_absolute() {
            return;
        }
        let path = PathBuf::from(cwd);
        let mut roots = self.granted_read_roots.lock();
        if let Some(pos) = roots.iter().position(|p| p == &path) {
            let refreshed = roots.remove(pos);
            roots.push(refreshed);
            return;
        }
        roots.push(path);
        if roots.len() > Self::MAX_GRANTED_CWD_ROOTS {
            roots.remove(0);
        }
    }

    /// A declared credential file stays unreadable to the plugin even when a
    /// read root covers its directory — the host reads it instead.
    pub(super) fn is_denied_read(&self, path: &str) -> bool {
        if self.denied_reads.is_empty() {
            return false;
        }
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
        self.denied_reads.iter().any(|denied| {
            let denied = std::fs::canonicalize(denied).unwrap_or_else(|_| denied.clone());
            denied == target
        })
    }

    /// `envGet` policy: unrestricted plugins read anything; confined ones can't
    /// read secret-looking vars (token/key/secret/password/credential/session).
    /// The heuristic is a substring match, so it cannot tell a host-set path
    /// variable from a secret — `HOST_PROVIDED_ENV` names the non-secret ones
    /// the host itself exports and is consulted first.
    pub(super) fn can_env(&self, name: &str) -> bool {
        if self.unrestricted {
            return true;
        }
        if HOST_PROVIDED_ENV.contains(&name) {
            return true;
        }
        let n = name.to_ascii_uppercase();
        let allowed = ![
            "TOKEN",
            "KEY",
            "SECRET",
            "PASSWORD",
            "PASSWD",
            "CREDENTIAL",
            "AUTH",
            "SESSION",
        ]
        .iter()
        .any(|needle| n.contains(needle));
        if !allowed {
            self.record_denial();
        }
        allowed
    }
}

/// Environment variables the HOST sets for plugins and that carry no secret —
/// filesystem paths and roots. Exact names only: a prefix rule would readmit
/// `CODETERM_API_TOKEN`, which shares the `CODETERM_` prefix with all of these.
/// Every entry here must be host-written and non-secret; a plugin-authored or
/// user-authored variable does not belong on this list.
pub(super) const HOST_PROVIDED_ENV: &[&str] = &[
    "CODETERM_CODEX_SESSIONS_ROOT",
    "CODETERM_CODEX_SESSION_INDEX_PATH",
    "CODETERM_CODEX_STATE_DB_PATH",
];

mod install;
pub(super) use install::install;

fn prepare_fetch(sandbox: &PluginSandbox, opts_json: &str) -> Result<String, String> {
    let host = super::host_net::host_of(opts_json).unwrap_or_default();
    if !sandbox.can_reach(&host) {
        return Err("fetch denied".to_string());
    }
    let prepared = super::host_credentials::inject(sandbox.credentials(), opts_json)?;
    let token = if sandbox.token_mode == TokenMode::None || !sandbox.supporter_token_allowed(&host)
    {
        None
    } else {
        sandbox
            .service()
            .and_then(|service| service.supporter_token(sandbox.plugin_id()))
    };
    super::host_credentials::inject_supporter_token(sandbox.token_mode, token.as_deref(), &prepared)
}

/// Wall-clock duration since UNIX epoch. Clock-set-before-1970 returns
/// zero rather than panic — keeps the plugin VM resilient to bad clocks.
fn since_epoch() -> std::time::Duration {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
}

// ── Implementations ───────────────────────────────────────────────────

/// Read at most `max_bytes` from the head (`tail=false`) or tail (`tail=true`)
/// of a file, trimming any partial UTF-8 sequence at the cut boundary so the
/// result is always valid UTF-8. Read-only: opens without create/truncate and
/// never touches the file's contents — session files stay untouched.
fn read_file_bounded(path: &str, max_bytes: u64, tail: bool) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let want = max_bytes.min(len);
    let cut_front = tail && want < len;
    if tail && want > 0 {
        f.seek(SeekFrom::Start(len - want)).ok()?;
    }
    let mut buf = vec![0u8; want as usize];
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = f.read(&mut buf[filled..]).ok()?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Some(utf8_trim_partial(&buf, cut_front))
}

/// An unreadable file is `Ok(None)`, the SDK's `string | null`; only the size cap throws.
pub(super) fn read_plugin_file(path: &str) -> Result<Option<String>, String> {
    use std::io::Read;
    let Ok(file) = File::open(path) else {
        return Ok(None);
    };
    let Ok(size) = file.metadata().map(|metadata| metadata.len()) else {
        return Ok(None);
    };
    if size > PLUGIN_READ_FILE_LIMIT {
        return Err(format!("readFile limit is {PLUGIN_READ_FILE_LIMIT} bytes"));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    if file
        .take(PLUGIN_READ_FILE_LIMIT)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

/// Drop a partial UTF-8 sequence left dangling at a byte-cut boundary: leading
/// continuation bytes when `trim_front` (tail cut mid-char) and any incomplete
/// trailing sequence (head cut mid-char).
fn utf8_trim_partial(bytes: &[u8], trim_front: bool) -> String {
    let mut start = 0;
    if trim_front {
        while start < bytes.len() && (bytes[start] & 0xC0) == 0x80 {
            start += 1;
        }
    }
    let slice = &bytes[start..];
    match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(e) => String::from_utf8_lossy(&slice[..e.valid_up_to()]).into_owned(),
    }
}

fn read_dir_json(path: &str) -> Option<String> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut out: Vec<serde_json::Value> = Vec::new();
    for e in entries.flatten() {
        out.push(dir_entry_to_json(
            &e.path(),
            &e.file_name().to_string_lossy(),
            e.metadata().ok(),
        ));
    }
    serde_json::to_string(&out).ok()
}

/// Immediate child DIRECTORY names only — no files, no recursion, no metadata.
fn list_child_dirs_json(path: &str) -> Option<String> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    serde_json::to_string(&names).ok()
}

/// Entries one walk may visit. The depth limit alone bounds nothing on a wide
/// tree: a provider session root with tens of thousands of files was walked on
/// every poll, pegging a long-lived daemon at 95-120% CPU and starving the mesh
/// relay behind it (#319). A truncated walk is reported, never silently short.
const WALK_ENTRY_BUDGET: usize = 20_000;

fn walk_dir_json(path: &str, max_depth: usize) -> Option<String> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    let started = std::time::Instant::now();
    let mut budget = WALK_ENTRY_BUDGET;
    walk_recursive(Path::new(path), 0, max_depth, &mut out, &mut budget);
    let elapsed = started.elapsed();
    if budget == 0 {
        log::warn!(
            "[host-api] walk of {path} hit the {WALK_ENTRY_BUDGET}-entry budget after {}ms;              the result is truncated",
            elapsed.as_millis()
        );
    } else if elapsed >= std::time::Duration::from_millis(250) {
        log::warn!(
            "[host-api] walk of {path} visited {} entries in {}ms",
            out.len(),
            elapsed.as_millis()
        );
    }
    serde_json::to_string(&out).ok()
}

fn walk_recursive(
    path: &Path,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<serde_json::Value>,
    budget: &mut usize,
) {
    if depth > max_depth || *budget == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for e in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        // `DirEntry::metadata` reuses what the directory scan already read where
        // the platform allows it; `fs::metadata` always pays a fresh lookup.
        out.push(dir_entry_to_json(&p, &name, e.metadata().ok()));
        if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            walk_recursive(&p, depth + 1, max_depth, out, budget);
        }
    }
}

/// `meta` is the caller's already-read metadata where it had one; `None` costs
/// a fresh lookup, which is why the walk passes what the scan gave it.
fn dir_entry_to_json(p: &Path, name: &str, meta: Option<std::fs::Metadata>) -> serde_json::Value {
    let meta = meta.or_else(|| std::fs::metadata(p).ok());
    let is_file = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
    let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
    let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
    let modified_ms = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    let created_ms = meta
        .as_ref()
        .and_then(|m| m.created().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    serde_json::json!({
        "name": name,
        "path": p.to_string_lossy(),
        "isFile": is_file,
        "isDir": is_dir,
        "size": size,
        "modifiedMs": modified_ms,
        "createdMs": created_ms,
    })
}

fn posix_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Cross-platform "set env var, then run the next command" prefix. POSIX
/// uses inline assignment (`NAME=value cmd`) so no statement-joiner is
/// needed at all — Windows PowerShell has no equivalent single-command
/// scoping, so it sets `$env:` and joins with `;` like the rest of this file.
fn shell_env_prefix(name: &str, value: &str, platform: &str) -> String {
    match platform {
        "windows" => format!("$env:{}={}; ", name, powershell_quote(value)),
        _ => format!("{}={} ", name, posix_single_quote(value)),
    }
}

fn powershell_quote(s: &str) -> String {
    // PowerShell single-quoted string: only single-quotes need doubling.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push('\'');
            out.push('\'');
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

// Name of the global the host object is installed under (see install above).
pub(super) const HOST_GLOBAL: &str = "host";

#[cfg(test)]
mod tests;
