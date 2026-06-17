//! `host` global injected into every plugin VM.
//!
//! This is the bridge that lets plugins do everything the native
//! Rust providers used to: filesystem I/O, sqlite reads, OS-aware
//! shell quoting, env, logging. The whole point of the JS plugin
//! architecture is that **all per-provider logic lives in TS**;
//! anything Rust still does has to be a generic primitive that ANY
//! plugin can call, not provider-specific helpers.
//!
//! All functions are **synchronous** — QuickJS calls block their VM,
//! so do not call anything here that blocks for noticeable time on
//! the hot path. File reads of session JSONs (kilobytes) and sqlite
//! point-queries fit. Network calls do not — they belong elsewhere.
//!
//! Return shapes:
//! * primitives (string / bool / i64) are returned directly
//! * complex data is returned as a JSON string the plugin parses with
//!   `JSON.parse(host.readDirJson(...))` — keeps the bridge boring.
//!
//! Errors are signalled by `null` returns (no exceptions across the
//! FFI boundary). If a plugin needs the failure reason, surface it
//! via a sibling `host.lastError()` accessor later.
//!
//! Safety: bundled + in-workspace plugins (ship with the app / dev repo) get
//! the full surface. A plugin dropped into `~/.codeterm/plugins/` is confined
//! by a `PluginSandbox` — FS/sqlite scoped to its own dir + declared session
//! root, `envGet` blocked for secret-looking names. Per-permission grants
//! (`permissions.fs.read` in `plugin.json`) can refine this later.

/// Version of the plugin/host contract this build implements. Third-party
/// drop-ins must declare a matching `hostApi` in their manifest or they are
/// skipped at load (`loader::validate_conformance`) instead of being registered
/// against an interface they don't conform to (#16). Bump on any
/// breaking change to the host surface or the `AgentProvider` capability set.
pub const HOST_API_VERSION: u32 = 2;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use rquickjs::function::Func;
use rquickjs::{CatchResultExt, Ctx};

use super::manifest::PluginManifest;
use super::vm::VmError;
use crate::service::ServiceContext;

/// A write-once, weakly-held reference resolved lazily. The plugin VM (and its
/// `PluginSandbox`) are constructed **before** the host `ServiceContext` exists
/// (lib.rs ordering — plan §1.5 F4), so the ref can't be a constructor argument:
/// it is populated after the fact via [`LateRef::set`] and read with
/// [`LateRef::get`]. `Weak` so a sandbox that outlives daemon teardown can never
/// pin the service alive.
pub(super) struct LateRef<T>(OnceLock<Weak<T>>);

impl<T> LateRef<T> {
    pub(super) fn new() -> Self {
        Self(OnceLock::new())
    }

    /// Populate the ref. First write wins (OnceLock); returns whether this call
    /// set it (`false` if it was already populated).
    pub(super) fn set(&self, weak: Weak<T>) -> bool {
        self.0.set(weak).is_ok()
    }

    /// Resolve to a live `Arc` if the ref was populated and the target is still
    /// alive; `None` before population or after the target has been dropped.
    pub(super) fn get(&self) -> Option<Arc<T>> {
        self.0.get().and_then(Weak::upgrade)
    }
}

/// Filesystem/env policy applied to a plugin's `host.*` calls. Bundled and
/// in-workspace plugins (ship with the app / the dev repo) are `unrestricted`.
/// A plugin dropped into the user plugins dir (`~/.codeterm/plugins/`) is
/// confined: it may only read/walk/sqlite under `read_roots` (its own dir +
/// the session dir it declares), only write under `write_roots`, and `envGet`
/// is blocked for secret-looking names — so an untrusted drop-in can't slurp
/// `~/.ssh`, the API token from config, or cloud-key env vars, nor write
/// autostart/integration files.
pub(super) struct PluginSandbox {
    unrestricted: bool,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    /// Plugin id — used to namespace the per-plugin secret bucket. Always set by
    /// the loader (even for unrestricted plugins) so `host.secret*` is isolated.
    plugin_id: String,
    /// `host.exec` binary allowlist (host_api v2). Empty unless declared.
    exec_allow: Vec<String>,
    /// `host.fetch` host allowlist (host_api v2). Empty unless declared.
    net_allow: Vec<String>,
    /// Whether `host.secret*` is permitted.
    secrets_allowed: bool,
    /// The plugin's own settings blob (`config.plugins.<id>`), as JSON. Returned
    /// by `host.settingsJson()` so a plugin reads its user-configured settings
    /// instead of env hacks. Defaults to `"null"`.
    settings_json: String,
    /// Late-bound handle to the host `ServiceContext`, populated after service
    /// init (the VM is built before `ServiceContext` exists — plan §1.5 F4).
    /// Carrier only at this stage: `host.worker`/`agent`/`mem` handles that read
    /// it land in a later track.
    svc: LateRef<ServiceContext>,
}

impl PluginSandbox {
    /// Full host access — bundled/workspace plugins and tests.
    pub(super) fn unrestricted() -> Self {
        Self {
            unrestricted: true,
            read_roots: Vec::new(),
            write_roots: Vec::new(),
            plugin_id: String::new(),
            exec_allow: Vec::new(),
            net_allow: Vec::new(),
            secrets_allowed: true,
            settings_json: "null".to_string(),
            svc: LateRef::new(),
        }
    }

    pub(super) fn restricted(read_roots: Vec<PathBuf>, write_roots: Vec<PathBuf>) -> Self {
        Self {
            unrestricted: false,
            read_roots,
            write_roots,
            plugin_id: String::new(),
            exec_allow: Vec::new(),
            net_allow: Vec::new(),
            secrets_allowed: false,
            settings_json: "null".to_string(),
            svc: LateRef::new(),
        }
    }

    /// Stamp the plugin identity + declared host_api-v2 permissions onto a base
    /// sandbox. Unrestricted plugins keep full exec/net/secrets; restricted ones
    /// get exactly what their manifest declares.
    pub(super) fn with_identity_and_perms(
        mut self,
        plugin_id: &str,
        exec_allow: Vec<String>,
        net_allow: Vec<String>,
        secrets: bool,
    ) -> Self {
        self.plugin_id = plugin_id.to_string();
        self.exec_allow = exec_allow;
        self.net_allow = net_allow;
        self.secrets_allowed = self.unrestricted || secrets;
        self
    }

    pub(super) fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// May the plugin run `bin` via `host.exec`?
    pub(super) fn can_exec(&self, bin: &str) -> bool {
        self.unrestricted || self.exec_allow.iter().any(|b| b == bin)
    }

    /// May the plugin reach `host` (host or `host:port`) via `host.fetch`?
    pub(super) fn can_reach(&self, host: &str) -> bool {
        if self.unrestricted {
            return true;
        }
        let host = host.trim();
        self.net_allow.iter().any(|allowed| {
            allowed == host
                // allow `host` to match an `host:port` rule and vice-versa
                || host.split(':').next() == Some(allowed.as_str())
                || allowed.split(':').next() == Some(host)
        })
    }

    pub(super) fn secrets_allowed(&self) -> bool {
        self.secrets_allowed
    }

    /// Attach the plugin's settings blob (`config.plugins.<id>` as JSON).
    pub(super) fn with_settings(mut self, settings_json: String) -> Self {
        self.settings_json = settings_json;
        self
    }

    pub(super) fn settings_json(&self) -> &str {
        &self.settings_json
    }

    /// Late-bind the host `ServiceContext` (see [`LateRef`]). First write wins.
    pub(super) fn set_service(&self, svc: Weak<ServiceContext>) -> bool {
        self.svc.set(svc)
    }

    /// Resolve the host `ServiceContext`, or `None` if not yet populated / gone.
    pub(super) fn service(&self) -> Option<Arc<ServiceContext>> {
        self.svc.get()
    }

    fn path_under(roots: &[PathBuf], path: &str) -> bool {
        // Compare canonical forms so `..` and symlinks can't escape. Fall back
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

    fn can_read(&self, path: &str) -> bool {
        self.unrestricted || Self::path_under(&self.read_roots, path)
    }

    fn can_write(&self, path: &str) -> bool {
        self.unrestricted || Self::path_under(&self.write_roots, path)
    }

    /// `envGet` policy: unrestricted plugins read anything; confined ones can't
    /// read secret-looking vars (token/key/secret/password/credential/session).
    fn can_env(&self, name: &str) -> bool {
        if self.unrestricted {
            return true;
        }
        let n = name.to_ascii_uppercase();
        ![
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
        .any(|needle| n.contains(needle))
    }
}

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

    // ── Identity / paths ────────────────────────────────────────────
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

    // ── Filesystem (path-gated for confined plugins) ────────────────
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
    bind!("walkDirJson", move |path: String,
                               max_depth: i32|
          -> Option<String> {
        if !sb.can_read(&path) {
            return None;
        }
        walk_dir_json(&path, max_depth.max(0) as usize)
    });

    // ── Shell / process help ────────────────────────────────────────
    bind!("shellQuote", |s: String| -> String {
        posix_single_quote(&s)
    });
    bind!("shellQuoteFor", |s: String, platform: String| -> String {
        match platform.as_str() {
            "windows" => powershell_quote(&s),
            _ => posix_single_quote(&s),
        }
    });
    bind!("writeTempFile", |contents: String,
                            suffix: Option<String>|
     -> Option<String> {
        write_temp_file(&contents, suffix.as_deref())
    });

    // Crypto helpers.
    // MD5 hex digest of a string. Use case: cursor stores chats under
    // `~/.cursor/chats/<md5(cwd)>/...`, so the plugin needs MD5 to resolve which chat dir belongs to the current pane's cwd.
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

    // ── Sqlite (read-only, db path gated for confined plugins) ──────
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

    // ── Logging ─────────────────────────────────────────────────────
    bind!("log", |level: String, msg: String| {
        match level.as_str() {
            "trace" => log::trace!("[plugin] {msg}"),
            "debug" => log::debug!("[plugin] {msg}"),
            "warn" => log::warn!("[plugin] {msg}"),
            "error" => log::error!("[plugin] {msg}"),
            _ => log::info!("[plugin] {msg}"),
        }
    });

    // ── host_api v2: subprocess / network / per-plugin secrets ──────
    // Each is gated by the manifest permission baked into the sandbox.
    let sb = sandbox.clone();
    bind!("settingsJson", move || -> String {
        sb.settings_json().to_string()
    });

    let sb = sandbox.clone();
    bind!("exec", move |opts_json: String| -> String {
        let bin = super::host_exec::bin_of(&opts_json).unwrap_or_default();
        if !sb.can_exec(&bin) {
            log::warn!("[plugin] denied exec({bin}) — not in subprocess.allow");
            return serde_json::json!({ "error": format!("exec denied: {bin}") }).to_string();
        }
        super::host_exec::run(&opts_json)
    });

    let sb = sandbox.clone();
    bind!("fetch", move |opts_json: String| -> String {
        let host = super::host_net::host_of(&opts_json).unwrap_or_default();
        if !sb.can_reach(&host) {
            log::warn!("[plugin] denied fetch({host}) — not in network.allow");
            return serde_json::json!({ "error": format!("fetch denied: {host}") }).to_string();
        }
        super::host_net::run(&opts_json)
    });

    let sb = sandbox.clone();
    bind!("secretGet", move |name: String| -> Option<String> {
        if !sb.secrets_allowed() {
            return None;
        }
        super::host_secret::get(sb.plugin_id(), &name)
    });
    let sb = sandbox.clone();
    bind!("secretSet", move |name: String, value: String| -> bool {
        if !sb.secrets_allowed() {
            return false;
        }
        super::host_secret::set(sb.plugin_id(), &name, &value)
    });
    let sb = sandbox.clone();
    bind!("secretDelete", move |name: String| -> bool {
        if !sb.secrets_allowed() {
            return false;
        }
        super::host_secret::delete(sb.plugin_id(), &name)
    });

    ctx.globals()
        .set("host", host)
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

fn read_dir_json(path: &str) -> Option<String> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut out: Vec<serde_json::Value> = Vec::new();
    for e in entries.flatten() {
        out.push(dir_entry_to_json(
            &e.path(),
            &e.file_name().to_string_lossy(),
        ));
    }
    serde_json::to_string(&out).ok()
}

fn walk_dir_json(path: &str, max_depth: usize) -> Option<String> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    walk_recursive(Path::new(path), 0, max_depth, &mut out);
    serde_json::to_string(&out).ok()
}

fn walk_recursive(path: &Path, depth: usize, max_depth: usize, out: &mut Vec<serde_json::Value>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        out.push(dir_entry_to_json(&p, &name));
        if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            walk_recursive(&p, depth + 1, max_depth, out);
        }
    }
}

fn dir_entry_to_json(p: &Path, name: &str) -> serde_json::Value {
    let meta = std::fs::metadata(p).ok();
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

// Re-exported for VM bootstrap to know the host object is installed.
#[allow(dead_code)]
pub(super) const HOST_GLOBAL: &str = "host";

#[allow(dead_code)]
fn _link_path_buf_unused(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::js_runtime::vm::PluginVm;
    use serde_json::json;

    fn vm_with(plugin_src: &str) -> PluginVm {
        PluginVm::from_source("test", plugin_src.to_string()).expect("vm")
    }

    #[test]
    fn sandbox_unrestricted_allows_everything() {
        let sb = PluginSandbox::unrestricted();
        assert!(sb.can_read("/etc/passwd"));
        assert!(sb.can_write("/anywhere"));
        assert!(sb.can_env("AWS_SECRET_ACCESS_KEY"));
    }

    #[test]
    fn sandbox_restricted_scopes_fs_and_blocks_secret_env() {
        let tmp = tempfile::tempdir().unwrap();
        let allowed = tmp.path().join("sessions");
        std::fs::create_dir_all(&allowed).unwrap();
        let sb = PluginSandbox::restricted(vec![allowed.clone()], vec![allowed.clone()]);

        // Inside the allowed root → ok.
        let inside = allowed.join("a.jsonl");
        std::fs::write(&inside, "{}").unwrap();
        assert!(sb.can_read(inside.to_str().unwrap()));
        assert!(sb.can_write(allowed.join("b.jsonl").to_str().unwrap()));

        // Outside → denied (incl. traversal attempts).
        assert!(!sb.can_read("/etc/passwd"));
        assert!(!sb.can_read(allowed.join("../../../../etc/passwd").to_str().unwrap()));
        assert!(!sb.can_write("/tmp/evil"));

        // envGet: path-ish vars allowed, secret-named blocked.
        assert!(sb.can_env("XDG_DATA_HOME"));
        assert!(sb.can_env("HOME"));
        assert!(!sb.can_env("CODETERM_API_TOKEN"));
        assert!(!sb.can_env("AWS_SECRET_ACCESS_KEY"));
        assert!(!sb.can_env("ANTHROPIC_API_KEY"));
        assert!(!sb.can_env("GITHUB_PASSWORD"));
    }

    #[test]
    fn host_api_v2_gating_by_allowlist() {
        // Restricted plugin with a narrow grant.
        let sb = PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms(
            "myplug",
            vec!["bw".into()],
            vec!["127.0.0.1:7891".into()],
            true,
        );
        assert_eq!(sb.plugin_id(), "myplug");
        // exec: only the allowlisted binary.
        assert!(sb.can_exec("bw"));
        assert!(!sb.can_exec("rm"));
        // fetch: host or host:port both match the rule.
        assert!(sb.can_reach("127.0.0.1:7891"));
        assert!(sb.can_reach("127.0.0.1"));
        assert!(!sb.can_reach("evil.example.com"));
        assert!(sb.secrets_allowed());

        // Restricted plugin with NO grants → everything denied.
        let none = PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms(
            "p2",
            vec![],
            vec![],
            false,
        );
        assert!(!none.can_exec("bw"));
        assert!(!none.can_reach("127.0.0.1:7891"));
        assert!(!none.secrets_allowed());

        // Unrestricted (bundled/dev) → everything allowed regardless of lists.
        let full =
            PluginSandbox::unrestricted().with_identity_and_perms("sys", vec![], vec![], false);
        assert!(full.can_exec("anything"));
        assert!(full.can_reach("anywhere"));
        assert!(full.secrets_allowed());
        assert_eq!(full.plugin_id(), "sys");
    }

    #[test]
    fn late_ref_resolves_only_after_population_while_target_lives() {
        // `LateRef` is the carrier `PluginSandbox.svc` uses. `ServiceContext`
        // cannot be built in a unit test (needs a `tauri::AppHandle` + ~40
        // fields), so resolution semantics are proven here against a stand-in
        // target; `PluginSandbox` delegates verbatim to this type.
        struct FakeService;
        let r: LateRef<FakeService> = LateRef::new();
        // Before population → None.
        assert!(r.get().is_none(), "unpopulated LateRef must not resolve");

        let svc = Arc::new(FakeService);
        assert!(r.set(Arc::downgrade(&svc)), "first set must succeed");
        // After population, target alive → Some, and it is the same Arc.
        let resolved = r.get().expect("populated LateRef must resolve");
        assert!(Arc::ptr_eq(&resolved, &svc));

        // First-write-wins: a second set is rejected and does not swap the ref.
        let other = Arc::new(FakeService);
        assert!(!r.set(Arc::downgrade(&other)), "second set must be rejected");
        assert!(Arc::ptr_eq(&r.get().unwrap(), &svc));

        // Drop every strong owner → the weak ref no longer upgrades.
        drop(resolved);
        drop(svc);
        assert!(r.get().is_none(), "LateRef must not resolve a dropped target");
    }

    #[test]
    fn sandbox_service_ref_is_late_bound() {
        // A fresh sandbox has no service ref. `set_service` with a live weak
        // makes `service()` resolve; this exercises the real `PluginSandbox`
        // carrier wiring against the real `ServiceContext` type.
        let sb = PluginSandbox::unrestricted();
        assert!(sb.service().is_none(), "sandbox starts with no service ref");
        // A dangling weak (no backing Arc) populates the slot but never resolves.
        assert!(sb.set_service(Weak::<ServiceContext>::new()));
        assert!(sb.service().is_none(), "dead weak ref must resolve to None");
        // Carrier is write-once.
        assert!(!sb.set_service(Weak::<ServiceContext>::new()));
    }

    #[test]
    fn host_settings_json_carries_plugin_config() {
        let sb = PluginSandbox::unrestricted().with_settings(r#"{"engine":"local"}"#.to_string());
        assert_eq!(sb.settings_json(), r#"{"engine":"local"}"#);
        // default is the JSON null literal so JSON.parse() in the plugin succeeds.
        assert_eq!(PluginSandbox::unrestricted().settings_json(), "null");
    }

    #[test]
    fn host_platform_matches_native() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                getPlatform() { return host.platform(); }
            };
        "#,
        );
        let p: String = crate::plugin::js_runtime::vm::call_json(&vm, "getPlatform", &[]).unwrap();
        assert_eq!(p, std::env::consts::OS);
    }

    #[test]
    fn host_home_dir_returns_path() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                getHome() { return host.homeDir(); }
            };
        "#,
        );
        let p: Option<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "getHome", &[]).unwrap();
        assert_eq!(
            p,
            dirs::home_dir().map(|p| p.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn host_expand_home_resolves_tilde() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                expand(p) { return host.expandHome(p); }
            };
        "#,
        );
        let p: Option<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "expand", &[json!("~/foo/bar")]).unwrap();
        if let Some(home) = dirs::home_dir() {
            assert_eq!(p, Some(home.join("foo/bar").to_string_lossy().into_owned()));
        }
    }

    #[test]
    fn host_write_then_read_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt").to_string_lossy().into_owned();
        let vm = vm_with(
            r#"
            module.exports.default = {
                write(p, c) { return host.writeFile(p, c); },
                read(p) { return host.readFile(p); },
                exists(p) { return host.fileExists(p); }
            };
        "#,
        );
        let ok: bool = crate::plugin::js_runtime::vm::call_json(
            &vm,
            "write",
            &[json!(path), json!("plugin wrote this")],
        )
        .unwrap();
        assert!(ok);
        let exists: bool =
            crate::plugin::js_runtime::vm::call_json(&vm, "exists", &[json!(path)]).unwrap();
        assert!(exists);
        let body: Option<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "read", &[json!(path)]).unwrap();
        assert_eq!(body.as_deref(), Some("plugin wrote this"));
    }

    #[test]
    fn host_write_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir
            .path()
            .join("a/b/c/hello.txt")
            .to_string_lossy()
            .into_owned();
        let vm = vm_with(
            r#"
            module.exports.default = {
                write(p, c) { return host.writeFile(p, c); }
            };
        "#,
        );
        let ok: bool = crate::plugin::js_runtime::vm::call_json(
            &vm,
            "write",
            &[json!(nested), json!("nested")],
        )
        .unwrap();
        assert!(ok);
        assert_eq!(
            std::fs::read_to_string(&nested).unwrap(),
            "nested".to_string()
        );
    }

    #[test]
    fn host_read_dir_json_lists_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "bb").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let vm = vm_with(
            r#"
            module.exports.default = {
                list(p) {
                    const j = host.readDirJson(p);
                    return JSON.parse(j).map(e => ({name: e.name, isDir: e.isDir}))
                        .sort((x, y) => x.name.localeCompare(y.name));
                }
            };
        "#,
        );
        let entries: Vec<serde_json::Value> =
            crate::plugin::js_runtime::vm::call_json(&vm, "list", &[json!(path)]).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0]["name"], "a.txt");
        assert_eq!(entries[0]["isDir"], false);
        assert_eq!(entries[2]["name"], "sub");
        assert_eq!(entries[2]["isDir"], true);
    }

    #[test]
    fn host_walk_dir_json_depth_bounded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        std::fs::write(dir.path().join("a/x.txt"), "x").unwrap();
        std::fs::write(dir.path().join("a/b/y.txt"), "y").unwrap();
        std::fs::write(dir.path().join("a/b/c/z.txt"), "z").unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let vm = vm_with(
            r#"
            module.exports.default = {
                walk(p, d) { return JSON.parse(host.walkDirJson(p, d)).map(e => e.name); }
            };
        "#,
        );
        let names1: Vec<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "walk", &[json!(path), json!(1)])
                .unwrap();
        // depth=1 should see "a", and immediate children ("b","x.txt").
        assert!(names1.contains(&"a".into()));
        assert!(names1.contains(&"x.txt".into()));
        assert!(
            !names1.contains(&"z.txt".into()),
            "depth=1 must not reach z.txt"
        );

        let names_deep: Vec<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "walk", &[json!(path), json!(5)])
                .unwrap();
        assert!(names_deep.contains(&"z.txt".into()));
    }

    #[test]
    fn host_shell_quote_posix_escapes_single_quote() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                q(s) { return host.shellQuote(s); }
            };
        "#,
        );
        let out: String =
            crate::plugin::js_runtime::vm::call_json(&vm, "q", &[json!("don't")]).unwrap();
        assert_eq!(out, "'don'\\''t'");
    }

    #[test]
    fn host_shell_quote_for_windows_uses_double_single_quote() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                q(s) { return host.shellQuoteFor(s, "windows"); }
            };
        "#,
        );
        let out: String =
            crate::plugin::js_runtime::vm::call_json(&vm, "q", &[json!("can't")]).unwrap();
        assert_eq!(out, "'can''t'");
    }

    #[test]
    fn host_write_temp_file_returns_real_path() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                mk(c) { return host.writeTempFile(c, ".task.txt"); }
            };
        "#,
        );
        let path: Option<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "mk", &[json!("payload payload")])
                .unwrap();
        let path = path.unwrap();
        assert!(path.ends_with(".task.txt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "payload payload");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn host_unix_now_monotonic_ish() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                now() { return host.unixNow(); },
                nowMs() { return host.unixNowMs(); }
            };
        "#,
        );
        let s: i64 = crate::plugin::js_runtime::vm::call_json(&vm, "now", &[]).unwrap();
        let ms: i64 = crate::plugin::js_runtime::vm::call_json(&vm, "nowMs", &[]).unwrap();
        assert!(s > 1_700_000_000, "unix seconds should be a real timestamp");
        assert!(ms > s * 1_000 - 5_000 && ms < (s + 5) * 1_000);
    }

    #[test]
    fn host_log_does_not_crash() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                go() { host.log("info", "hello from plugin"); return 1; }
            };
        "#,
        );
        let r: i32 = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
        assert_eq!(r, 1);
    }

    #[test]
    fn host_sqlite_query_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE k(id INTEGER PRIMARY KEY, name TEXT, score REAL);\
                 INSERT INTO k VALUES (1,'alpha',1.5),(2,'beta',2.5),(3,'gamma',3.5);",
            )
            .unwrap();
        }
        let db_path = db.to_string_lossy().into_owned();
        let vm = vm_with(
            r#"
            module.exports.default = {
                topByName(p) {
                    return JSON.parse(host.sqliteQueryAllJson(
                        p, "SELECT id, name, score FROM k WHERE score > ?1 ORDER BY name DESC", "[2.0]"));
                }
            };
        "#,
        );
        let rows: Vec<serde_json::Value> =
            crate::plugin::js_runtime::vm::call_json(&vm, "topByName", &[json!(db_path)]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "gamma");
        assert_eq!(rows[0]["id"], 3);
        assert_eq!(rows[1]["name"], "beta");
    }

    #[test]
    fn host_sqlite_handles_missing_db_gracefully() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                run() { return host.sqliteQueryAllJson("/no/such/db.sqlite", "SELECT 1", null); }
            };
        "#,
        );
        let r: Option<String> = crate::plugin::js_runtime::vm::call_json(&vm, "run", &[]).unwrap();
        assert_eq!(r, None);
    }

    #[test]
    fn host_install_idempotent_across_reload() {
        let vm = vm_with(
            r#"
            module.exports.default = {
                go() { return host.platform(); }
            };
        "#,
        );
        let p1: String = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
        vm.reload(
            r#"module.exports.default = { go() { return host.platform() + "!"; } };"#.to_string(),
        )
        .unwrap();
        let p2: String = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
        assert_eq!(p2, format!("{p1}!"));
    }
}
