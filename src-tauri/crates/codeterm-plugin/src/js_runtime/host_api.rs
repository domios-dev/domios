//! `host` global injected into every plugin VM — the generic FS / sqlite /
//! shell-quoting / env / logging primitives that let all per-provider logic
//! live in TS.
//!
//! Contract: all functions are synchronous (QuickJS blocks their VM, so nothing
//! here may block on the hot path); complex data is returned as a JSON string;
//! errors are signalled by `null` (no exceptions across the FFI boundary).
//!
//! Safety: bundled/in-workspace plugins get the full surface; a drop-in under
//! `~/.codeterm/plugins/` is confined by `PluginSandbox` (FS/sqlite scoped to
//! its own dir + declared session root, secret-looking env vars blocked).

/// Version of the plugin/host contract; a drop-in must declare a matching `hostApi`
/// or it's skipped at load. Bump on any breaking change to the host or `AgentProvider` surface.
pub const HOST_API_VERSION: u32 = 2;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use rquickjs::function::Func;
use rquickjs::{CatchResultExt, Ctx};

use super::manifest::{CredentialConfig, PluginManifest};
use super::vm::VmError;
use crate::host_traits::PluginHostCtx;

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
    /// Whether `host.secret*` is permitted.
    secrets_allowed: bool,
    /// `agent_commands` grant — gates routing a spawned agent's `codeterm plugin <id> <verb>` to `onAgentCommand`.
    agent_commands_allowed: bool,
    /// Whether `host.workspace*` is permitted (`manage_workspaces` grant).
    manage_workspaces_allowed: bool,
    /// Whether `host.agent*` is permitted (`spawn_agents` grant).
    spawn_agents_allowed: bool,
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
            granted_read_roots: parking_lot::Mutex::new(Vec::new()),
            denied_reads: Vec::new(),
            credentials: Vec::new(),
            denials: std::sync::atomic::AtomicU64::new(0),
            plugin_id: String::new(),
            exec_allow: Vec::new(),
            net_allow: Vec::new(),
            secrets_allowed: true,
            agent_commands_allowed: true,
            manage_workspaces_allowed: true,
            spawn_agents_allowed: true,
            settings_json: parking_lot::Mutex::new("null".to_string()),
            svc: LateRef::new(),
        }
    }

    pub(super) fn restricted(read_roots: Vec<PathBuf>, write_roots: Vec<PathBuf>) -> Self {
        Self {
            unrestricted: false,
            read_roots,
            write_roots,
            granted_read_roots: parking_lot::Mutex::new(Vec::new()),
            denied_reads: Vec::new(),
            credentials: Vec::new(),
            denials: std::sync::atomic::AtomicU64::new(0),
            plugin_id: String::new(),
            exec_allow: Vec::new(),
            net_allow: Vec::new(),
            secrets_allowed: false,
            agent_commands_allowed: false,
            manage_workspaces_allowed: false,
            spawn_agents_allowed: false,
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
        self.manage_workspaces_allowed = self.unrestricted || perms.manage_workspaces;
        self.spawn_agents_allowed = self.unrestricted || perms.spawn_agents;
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
        if self.unrestricted {
            return true;
        }
        let host = host.trim();
        self.net_allow.iter().any(|allowed| {
            allowed == host
                || host.split(':').next() == Some(allowed.as_str())
                || allowed.split(':').next() == Some(host)
        })
    }

    pub(super) fn secrets_allowed(&self) -> bool {
        self.secrets_allowed
    }

    /// Whether the plugin holds the `agent_commands` grant (inverse-seam routing).
    pub(super) fn agent_commands_allowed(&self) -> bool {
        self.agent_commands_allowed
    }

    /// Whether the plugin may create/manage workspaces (`host.workspace*`).
    pub(super) fn manage_workspaces_allowed(&self) -> bool {
        self.manage_workspaces_allowed
    }

    /// Whether the plugin may spawn/drive agents (`host.agent*`).
    pub(super) fn spawn_agents_allowed(&self) -> bool {
        self.spawn_agents_allowed
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
        self.svc.get()
    }

    fn path_under(roots: &[PathBuf], path: &str) -> bool {
        // to the lexical path when the target doesn't exist yet (writes).
        let target = std::fs::canonicalize(path)
            .ok()
            .or_else(|| {
                Path::new(path)
                    .parent()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .map(|p| p.join(Path::new(path).file_name().unwrap_or_default()))
            })
            .unwrap_or_else(|| PathBuf::from(path));
        roots.iter().any(|root| {
            std::fs::canonicalize(root)
                .map(|r| target.starts_with(&r))
                .unwrap_or(false)
        })
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

    pub(super) fn can_read(&self, path: &str) -> bool {
        if self.is_denied_read(path) {
            self.record_denial();
            return false;
        }
        let allowed = self.unrestricted
            || Self::path_under(&self.read_roots, path)
            || Self::path_under(&self.granted_read_roots.lock(), path);
        if !allowed {
            self.record_denial();
        }
        allowed
    }

    pub(super) fn can_write(&self, path: &str) -> bool {
        let allowed = self.unrestricted || Self::path_under(&self.write_roots, path);
        if !allowed {
            self.record_denial();
        }
        allowed
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
    "CODETERM_CODEX_THREAD_HISTORY_PATH",
];

/// Install the `host` global into a fresh QuickJS context. Called by
/// `PluginVm::from_source` (and `reload`) before the plugin bundle
/// evaluates, so the plugin's top-level code can `host.platform()`
/// etc. immediately.
///
/// `manifest_json`, if present, is returned to the plugin by
/// `host.manifest()` — lets a plugin read its own declared paths
/// (rule.path, sessions.rootDir, …) without us threading them
/// through every call site.
pub(super) fn install(
    ctx: &Ctx<'_>,
    manifest_json: Option<&str>,
    sandbox: Arc<PluginSandbox>,
) -> Result<(), VmError> {
    let host = rquickjs::Object::new(ctx.clone())
        .map_err(|e| VmError::Script(format!("host obj: {e}")))?;

    // Tiny helper to fold the repeated `host.set(name, Func::new(closure)) .map_err(|e| VmError::Script(format!("set {name}: {e}")))?` shape.
    // Reads top-to-bottom as a flat table of (name, closure).
    macro_rules! bind {
        ($name:expr, $closure:expr) => {
            host.set($name, Func::new($closure))
                .map_err(|e| VmError::Script(format!("set {}: {e}", $name)))?
        };
    }

    let manifest_owned: Option<String> = manifest_json.map(|s| s.to_string());
    bind!("manifestJson", move || -> Option<String> {
        manifest_owned.clone()
    });

    bind!("platform", || -> String {
        std::env::consts::OS.to_string()
    });
    bind!("homeDir", || -> Option<String> {
        dirs::home_dir().map(|p| p.to_string_lossy().into_owned())
    });
    bind!("expandHome", |path: String| -> Option<String> {
        PluginManifest::expand_home(&path).map(|p| p.to_string_lossy().into_owned())
    });
    let sb = sandbox.clone();
    bind!("envGet", move |name: String| -> Option<String> {
        if !sb.can_env(&name) {
            log::warn!("[plugin] denied envGet({name}) for confined plugin");
            return None;
        }
        std::env::var(&name).ok()
    });
    bind!("unixNow", || -> i64 { since_epoch().as_secs() as i64 });
    bind!("unixNowMs", || -> i64 { since_epoch().as_millis() as i64 });

    let sb = sandbox.clone();
    bind!("fileExists", move |path: String| -> bool {
        sb.can_read(&path) && Path::new(&path).exists()
    });
    let sb = sandbox.clone();
    bind!("readFile", move |path: String| -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied readFile outside sandbox: {path}");
            return None;
        }
        std::fs::read_to_string(&path).ok()
    });
    let sb = sandbox.clone();
    bind!("readFileHead", move |path: String,
                                max_bytes: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied readFileHead outside sandbox: {path}");
            return None;
        }
        read_file_bounded(&path, max_bytes.max(0) as u64, false)
    });
    let sb = sandbox.clone();
    bind!("readFileTail", move |path: String,
                                max_bytes: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied readFileTail outside sandbox: {path}");
            return None;
        }
        read_file_bounded(&path, max_bytes.max(0) as u64, true)
    });
    let sb = sandbox.clone();
    bind!("writeFile", move |path: String, contents: String| -> bool {
        if !sb.can_write(&path) {
            log::warn!("[plugin] denied writeFile outside sandbox: {path}");
            return false;
        }
        let p = Path::new(&path);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        std::fs::write(p, contents).is_ok()
    });
    let sb = sandbox.clone();
    bind!("removeFile", move |path: String| -> bool {
        sb.can_write(&path) && std::fs::remove_file(&path).is_ok()
    });
    let sb = sandbox.clone();
    bind!("makeDirs", move |path: String| -> bool {
        sb.can_write(&path) && std::fs::create_dir_all(&path).is_ok()
    });
    let sb = sandbox.clone();
    bind!("readDirJson", move |path: String| -> Option<String> {
        if !sb.can_read(&path) {
            return None;
        }
        read_dir_json(&path)
    });
    let sb = sandbox.clone();
    bind!("listChildDirsJson", move |path: String| -> Option<String> {
        if !sb.can_read(&path) {
            log::warn!("[plugin] denied listChildDirs outside sandbox: {path}");
            return None;
        }
        list_child_dirs_json(&path)
    });
    let sb = sandbox.clone();
    bind!("walkDirJson", move |path: String,
                               max_depth: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            return None;
        }
        walk_dir_json(&path, max_depth.max(0) as usize)
    });

    bind!("shellQuote", |s: String| -> String {
        posix_single_quote(&s)
    });
    bind!("shellQuoteFor", |s: String, platform: String| -> String {
        match platform.as_str() {
            "windows" => powershell_quote(&s),
            _ => posix_single_quote(&s),
        }
    });
    bind!("shellEnvPrefix", |name: String,
                             value: String,
                             platform: String|
     -> String {
        shell_env_prefix(&name, &value, &platform)
    });
    bind!("writeTempFile", |contents: String,
                            suffix: Option<String>|
     -> Option<String> {
        write_temp_file(&contents, suffix.as_deref())
    });
    bind!("toolcallParse", |raw_text: String,
                            schema_json: String|
     -> String {
        super::toolcall_parse::parse_to_json(&raw_text, &schema_json)
    });

    // Crypto helpers.
    bind!("md5", |s: String| -> String {
        use md5::{Digest, Md5};
        let mut h = Md5::new();
        h.update(s.as_bytes());
        let digest = h.finalize();
        let mut out = String::with_capacity(32);
        for b in digest {
            out.push_str(&format!("{b:02x}"));
        }
        out
    });

    let sb = sandbox.clone();
    bind!("sqliteQueryAllJson", move |db: String,
                                      sql: String,
                                      params_json: Option<String>|
          -> Option<String> {
        if !sb.can_read(&db) {
            log::warn!("[plugin] denied sqliteQuery outside sandbox: {db}");
            return None;
        }
        sqlite_query_all_json(&db, &sql, params_json.as_deref())
    });

    bind!("log", |level: String, msg: String| {
        match level.as_str() {
            "trace" => log::trace!("[plugin] {msg}"),
            "debug" => log::debug!("[plugin] {msg}"),
            "warn" => log::warn!("[plugin] {msg}"),
            "error" => log::error!("[plugin] {msg}"),
            _ => log::info!("[plugin] {msg}"),
        }
    });

    // Each is gated by the manifest permission baked into the sandbox.
    let sb = sandbox.clone();
    bind!("settingsJson", move || -> String { sb.settings_json() });

    let sb = sandbox.clone();
    bind!("exec", move |opts_json: String| -> String {
        let bin = super::host_exec::bin_of(&opts_json).unwrap_or_default();
        if !sb.can_exec(&bin) {
            log::warn!(
                "[plugin:{}] denied exec({bin}) — not in subprocess.allow",
                sb.plugin_id()
            );
            return serde_json::json!({ "error": format!("exec denied: {bin}") }).to_string();
        }
        super::host_exec::run_with_host_env(&opts_json, &super::host_exec::current_host_env())
    });

    // Async exec (job-id + poll): same gate as `exec`, but the command runs off-thread so the VM lock is held only per start/poll.
    let sb = sandbox.clone();
    bind!("execStart", move |opts_json: String| -> String {
        let bin = super::host_exec::bin_of(&opts_json).unwrap_or_default();
        if !sb.can_exec(&bin) {
            log::warn!(
                "[plugin:{}] denied exec.start({bin}) — not in subprocess.allow",
                sb.plugin_id()
            );
            return serde_json::json!({ "error": format!("exec denied: {bin}") }).to_string();
        }
        super::host_exec::start_with_host_env(
            sb.plugin_id(),
            &opts_json,
            &super::host_exec::current_host_env(),
        )
    });
    let sb = sandbox.clone();
    bind!("execPoll", move |job_id: String| -> String {
        super::host_exec::poll(sb.plugin_id(), &job_id)
    });
    let sb = sandbox.clone();
    bind!("execClose", move |job_id: String| {
        super::host_exec::close(sb.plugin_id(), &job_id);
    });

    let sb = sandbox.clone();
    bind!("credentialPublic", move |id: String| -> Option<String> {
        super::host_credentials::public_json(sb.credentials(), &id)
    });
    let sb = sandbox.clone();
    bind!("fetch", move |opts_json: String| -> String {
        let host = super::host_net::host_of(&opts_json).unwrap_or_default();
        if !sb.can_reach(&host) {
            log::warn!("[plugin] denied fetch({host}) — not in network.allow");
            return serde_json::json!({ "error": format!("fetch denied: {host}") }).to_string();
        }
        match super::host_credentials::inject(sb.credentials(), &opts_json) {
            Ok(prepared) => super::host_net::run(&prepared),
            Err(e) => serde_json::json!({ "error": e }).to_string(),
        }
    });
    let sb = sandbox.clone();
    bind!("fetchStream", move |opts_json: String| -> String {
        let host = super::host_net::host_of(&opts_json).unwrap_or_default();
        // Egress-equivalent to host.fetch: use the exact same network.allow gate.
        if !sb.can_reach(&host) {
            log::warn!("[plugin] denied fetch.stream({host}) — not in network.allow");
            return serde_json::json!({ "error": format!("fetch denied: {host}") }).to_string();
        }
        super::host_net::stream(sb.plugin_id(), &opts_json)
    });
    let sb = sandbox.clone();
    bind!("fetchStreamPoll", move |job_id: String| -> String {
        super::host_net::stream_poll(sb.plugin_id(), &job_id)
    });
    let sb = sandbox.clone();
    bind!("fetchStreamClose", move |job_id: String| {
        super::host_net::stream_close(sb.plugin_id(), &job_id);
    });

    let sb = sandbox.clone();
    bind!("secretGet", move |name: String| -> Option<String> {
        if !sb.secrets_allowed() {
            return None;
        }
        sb.service()?.secret_get(sb.plugin_id(), &name)
    });
    let sb = sandbox.clone();
    bind!("secretSet", move |name: String, value: String| -> bool {
        if !sb.secrets_allowed() {
            return false;
        }
        sb.service()
            .map(|svc| svc.secret_set(sb.plugin_id(), &name, &value))
            .unwrap_or(false)
    });
    let sb = sandbox.clone();
    bind!("secretDelete", move |name: String| -> bool {
        if !sb.secrets_allowed() {
            return false;
        }
        sb.service()
            .map(|svc| svc.secret_delete(sb.plugin_id(), &name))
            .unwrap_or(false)
    });

    // host compute base (host.worker/agent/workspace/mem) — lives in `host_compute` to keep this bind table flat.
    super::host_compute::install_handles(&host, &sandbox)?;

    #[cfg(test)]
    super::call_gate::install(&host, &sandbox)?;

    ctx.globals()
        .set(HOST_GLOBAL, host)
        .catch(ctx)
        .map_err(|e| VmError::Script(format!("set host global: {e:?}")))?;
    Ok(())
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

fn write_temp_file(contents: &str, suffix: Option<&str>) -> Option<String> {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let suffix = suffix.unwrap_or("");
    let name = format!(
        "codeterm-plugin-{}{}",
        uuid::Uuid::new_v4().simple(),
        suffix,
    );
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).ok()?;
    f.write_all(contents.as_bytes()).ok()?;
    Some(path.to_string_lossy().into_owned())
}

fn sqlite_query_all_json(db_path: &str, sql: &str, params_json: Option<&str>) -> Option<String> {
    let params: Vec<serde_json::Value> = match params_json {
        Some(j) => serde_json::from_str(j).ok()?,
        None => Vec::new(),
    };
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let mut stmt = conn.prepare(sql).ok()?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| {
            stmt.column_name(i)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| format!("col_{i}"))
        })
        .collect();
    let bound: Vec<Box<dyn rusqlite::ToSql>> = params
        .into_iter()
        .map(|v| -> Box<dyn rusqlite::ToSql> {
            match v {
                serde_json::Value::Null => Box::new(rusqlite::types::Null),
                serde_json::Value::Bool(b) => Box::new(b),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Box::new(i)
                    } else if let Some(f) = n.as_f64() {
                        Box::new(f)
                    } else {
                        Box::new(n.to_string())
                    }
                }
                serde_json::Value::String(s) => Box::new(s),
                other => Box::new(other.to_string()),
            }
        })
        .collect();
    let bound_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
    let rows_iter = stmt
        .query_map(rusqlite::params_from_iter(bound_refs), |row| {
            let mut obj = serde_json::Map::with_capacity(col_count);
            for i in 0..col_count {
                let val: rusqlite::types::Value =
                    row.get(i).unwrap_or(rusqlite::types::Value::Null);
                let jv = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(i) => serde_json::Value::from(i),
                    rusqlite::types::Value::Real(f) => serde_json::Value::from(f),
                    rusqlite::types::Value::Text(s) => serde_json::Value::String(s),
                    rusqlite::types::Value::Blob(b) => {
                        use base64::Engine;
                        serde_json::Value::String(
                            base64::engine::general_purpose::STANDARD.encode(b),
                        )
                    }
                };
                obj.insert(col_names[i].clone(), jv);
            }
            Ok(serde_json::Value::Object(obj))
        })
        .ok()?;
    let rows: Vec<serde_json::Value> = rows_iter.flatten().collect();
    serde_json::to_string(&rows).ok()
}

// Name of the global the host object is installed under (see install above).
pub(super) const HOST_GLOBAL: &str = "host";

#[cfg(test)]
mod tests;
