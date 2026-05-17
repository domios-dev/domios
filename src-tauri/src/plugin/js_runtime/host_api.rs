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
//! Safety: every plugin gets the full surface today. Once we have a
//! third-party plugin marketplace, gate by `permissions.fs.read` etc.
//! in `plugin.json`. For now the trust boundary is "you installed the
//! plugin into `~/.codeterm/plugins/`, you trust it."

use std::path::{Path, PathBuf};

use rquickjs::function::Func;
use rquickjs::{CatchResultExt, Ctx};

use super::manifest::PluginManifest;
use super::vm::VmError;

/// Install the `host` global into a fresh QuickJS context. Called by
/// `PluginVm::from_source` (and `reload`) before the plugin bundle
/// evaluates, so the plugin's top-level code can `host.platform()`
/// etc. immediately.
///
/// `manifest_json`, if present, is returned to the plugin by
/// `host.manifest()` — lets a plugin read its own declared paths
/// (rule.path, sessions.rootDir, …) without us threading them
/// through every call site.
pub(super) fn install(ctx: &Ctx<'_>, manifest_json: Option<&str>) -> Result<(), VmError> {
    let g = ctx.globals();
    let host = rquickjs::Object::new(ctx.clone())
        .map_err(|e| VmError::Script(format!("host obj: {e}")))?;

    let manifest_owned: Option<String> = manifest_json.map(|s| s.to_string());
    host.set(
        "manifestJson",
        Func::new(move || -> Option<String> { manifest_owned.clone() }),
    )
    .map_err(|e| VmError::Script(format!("set manifestJson: {e}")))?;

    // ── Identity / paths ────────────────────────────────────────────

    host.set("platform", Func::new(|| -> String {
        std::env::consts::OS.to_string()
    }))
    .map_err(|e| VmError::Script(format!("set platform: {e}")))?;

    host.set("homeDir", Func::new(|| -> Option<String> {
        dirs::home_dir().map(|p| p.to_string_lossy().into_owned())
    }))
    .map_err(|e| VmError::Script(format!("set homeDir: {e}")))?;

    host.set("expandHome", Func::new(|path: String| -> Option<String> {
        PluginManifest::expand_home(&path)
            .map(|p| p.to_string_lossy().into_owned())
    }))
    .map_err(|e| VmError::Script(format!("set expandHome: {e}")))?;

    host.set("envGet", Func::new(|name: String| -> Option<String> {
        std::env::var(&name).ok()
    }))
    .map_err(|e| VmError::Script(format!("set envGet: {e}")))?;

    host.set("unixNow", Func::new(|| -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }))
    .map_err(|e| VmError::Script(format!("set unixNow: {e}")))?;

    host.set("unixNowMs", Func::new(|| -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }))
    .map_err(|e| VmError::Script(format!("set unixNowMs: {e}")))?;

    // ── Filesystem ──────────────────────────────────────────────────

    host.set("fileExists", Func::new(|path: String| -> bool {
        Path::new(&path).exists()
    }))
    .map_err(|e| VmError::Script(format!("set fileExists: {e}")))?;

    host.set("readFile", Func::new(|path: String| -> Option<String> {
        std::fs::read_to_string(&path).ok()
    }))
    .map_err(|e| VmError::Script(format!("set readFile: {e}")))?;

    host.set(
        "writeFile",
        Func::new(|path: String, contents: String| -> bool {
            let p = Path::new(&path);
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            std::fs::write(p, contents).is_ok()
        }),
    )
    .map_err(|e| VmError::Script(format!("set writeFile: {e}")))?;

    host.set("removeFile", Func::new(|path: String| -> bool {
        std::fs::remove_file(&path).is_ok()
    }))
    .map_err(|e| VmError::Script(format!("set removeFile: {e}")))?;

    host.set("makeDirs", Func::new(|path: String| -> bool {
        std::fs::create_dir_all(&path).is_ok()
    }))
    .map_err(|e| VmError::Script(format!("set makeDirs: {e}")))?;

    host.set(
        "readDirJson",
        Func::new(|path: String| -> Option<String> { read_dir_json(&path) }),
    )
    .map_err(|e| VmError::Script(format!("set readDirJson: {e}")))?;

    host.set(
        "walkDirJson",
        Func::new(|path: String, max_depth: i32| -> Option<String> {
            walk_dir_json(&path, max_depth.max(0) as usize)
        }),
    )
    .map_err(|e| VmError::Script(format!("set walkDirJson: {e}")))?;

    // ── Shell / process help ────────────────────────────────────────

    host.set("shellQuote", Func::new(|s: String| -> String {
        // Default = POSIX single-quote escape; plugin asks for OS-aware
        // form via `shellQuoteFor(..., platform)`.
        posix_single_quote(&s)
    }))
    .map_err(|e| VmError::Script(format!("set shellQuote: {e}")))?;

    host.set(
        "shellQuoteFor",
        Func::new(|s: String, platform: String| -> String {
            match platform.as_str() {
                "windows" => powershell_quote(&s),
                _ => posix_single_quote(&s),
            }
        }),
    )
    .map_err(|e| VmError::Script(format!("set shellQuoteFor: {e}")))?;

    host.set(
        "writeTempFile",
        Func::new(|contents: String, suffix: Option<String>| -> Option<String> {
            write_temp_file(&contents, suffix.as_deref())
        }),
    )
    .map_err(|e| VmError::Script(format!("set writeTempFile: {e}")))?;

    // ── Sqlite (read-only) ──────────────────────────────────────────

    host.set(
        "sqliteQueryAllJson",
        Func::new(
            |db_path: String, sql: String, params_json: Option<String>| -> Option<String> {
                sqlite_query_all_json(&db_path, &sql, params_json.as_deref())
            },
        ),
    )
    .map_err(|e| VmError::Script(format!("set sqliteQueryAllJson: {e}")))?;

    // ── Logging ─────────────────────────────────────────────────────

    host.set(
        "log",
        Func::new(|level: String, msg: String| {
            match level.as_str() {
                "trace" => log::trace!("[plugin] {msg}"),
                "debug" => log::debug!("[plugin] {msg}"),
                "warn" => log::warn!("[plugin] {msg}"),
                "error" => log::error!("[plugin] {msg}"),
                _ => log::info!("[plugin] {msg}"),
            }
        }),
    )
    .map_err(|e| VmError::Script(format!("set log: {e}")))?;

    g.set("host", host)
        .catch(ctx)
        .map_err(|e| VmError::Script(format!("set host global: {e:?}")))?;
    Ok(())
}

// ── Implementations ───────────────────────────────────────────────────

fn read_dir_json(path: &str) -> Option<String> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut out: Vec<serde_json::Value> = Vec::new();
    for e in entries.flatten() {
        out.push(dir_entry_to_json(&e.path(), &e.file_name().to_string_lossy()));
    }
    serde_json::to_string(&out).ok()
}

fn walk_dir_json(path: &str, max_depth: usize) -> Option<String> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    walk_recursive(Path::new(path), 0, max_depth, &mut out);
    serde_json::to_string(&out).ok()
}

fn walk_recursive(
    path: &Path,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<serde_json::Value>,
) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
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

fn sqlite_query_all_json(
    db_path: &str,
    sql: &str,
    params_json: Option<&str>,
) -> Option<String> {
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
                let val: rusqlite::types::Value = row.get(i).unwrap_or(rusqlite::types::Value::Null);
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
        assert_eq!(p, dirs::home_dir().map(|p| p.to_string_lossy().into_owned()));
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
            crate::plugin::js_runtime::vm::call_json(&vm, "expand", &[json!("~/foo/bar")])
                .unwrap();
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
        assert!(!names1.contains(&"z.txt".into()), "depth=1 must not reach z.txt");

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
        let out: String = crate::plugin::js_runtime::vm::call_json(
            &vm,
            "q",
            &[json!("don't")],
        )
        .unwrap();
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
        let out: String = crate::plugin::js_runtime::vm::call_json(
            &vm,
            "q",
            &[json!("can't")],
        )
        .unwrap();
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
        let path: Option<String> = crate::plugin::js_runtime::vm::call_json(
            &vm,
            "mk",
            &[json!("payload payload")],
        )
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
        let r: Option<String> =
            crate::plugin::js_runtime::vm::call_json(&vm, "run", &[]).unwrap();
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
        vm.reload(r#"module.exports.default = { go() { return host.platform() + "!"; } };"#
            .to_string())
            .unwrap();
        let p2: String = crate::plugin::js_runtime::vm::call_json(&vm, "go", &[]).unwrap();
        assert_eq!(p2, format!("{p1}!"));
    }
}
