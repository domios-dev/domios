//! `PluginVm` — one QuickJS context per plugin.

use std::path::Path;

use parking_lot::Mutex;
use rquickjs::{CatchResultExt, Context, Function, Runtime, Value};

/// Convenience layer wrapped around the raw `host` API. Loaded into
/// every plugin VM right after `host_api::install` and before the
/// plugin's own code, so plugin authors get parsed objects out of
/// readDir/walkDir/sqliteQuery/manifest without writing JSON.parse
/// boilerplate at every call site.
///
/// Keep `host.readDirJson` / `host.walkDirJson` / `host.sqliteQueryAllJson`
/// / `host.manifestJson` reachable as raw escape hatches; the preamble
/// only **adds** properties, never shadows the raw ones.
const PLUGIN_PREAMBLE: &str = r#"
(function() {
  if (typeof host !== "object" || host === null) return;
  function parseJson(s) {
    if (s === null || s === undefined) return null;
    try { return JSON.parse(s); } catch (_) { return null; }
  }
  host.fs = {
    readFile:      host.readFile,
    writeFile:     host.writeFile,
    removeFile:    host.removeFile,
    makeDirs:      host.makeDirs,
    fileExists:    host.fileExists,
    expandHome:    host.expandHome,
    writeTempFile: host.writeTempFile,
    readDir: function (p) { return parseJson(host.readDirJson(p)) || []; },
    walkDir: function (p, maxDepth) {
      var d = (maxDepth === undefined || maxDepth === null) ? 8 : (maxDepth | 0);
      return parseJson(host.walkDirJson(p, d)) || [];
    },
    sqliteQuery: function (db, sql, params) {
      var pj = (params === undefined || params === null) ? null : JSON.stringify(params);
      return parseJson(host.sqliteQueryAllJson(db, sql, pj)) || [];
    },
    readJson: function (p) {
      var s = host.readFile(p);
      return s === null || s === undefined ? null : parseJson(s);
    },
    writeJson: function (p, value) {
      return host.writeFile(p, JSON.stringify(value, null, 2));
    },
  };
  host.shell = {
    quote:    host.shellQuote,
    quoteFor: host.shellQuoteFor,
  };
  host.toolcall = {
    parse: host.toolcallParse,
  };
  // Normalize/slurp path variants (native vs forwarded slash forms) before comparisons.
  // Session detection depends on semantic equality, not raw string equality.
  host.path = {
    isWindows: host.platform() === "windows",
    toForwardSlash: function (p) { return String(p || "").replace(/\\/g, "/"); },
    toNative: function (p) {
      var s = String(p || "");
      return host.path.isWindows ? s.replace(/\//g, "\\") : s.replace(/\\/g, "/");
    },
    normalize: function (p) {
      var s = String(p || "").replace(/\\/g, "/");
      return host.path.isWindows ? s.toLowerCase() : s;
    },
    equal: function (a, b) {
      return host.path.normalize(a) === host.path.normalize(b);
    },
  };
  host.manifest = function () { return parseJson(host.manifestJson()); };

  // ── host compute base ──────────────────────────────────────────────
  // ALL compute binds are SYNCHRONOUS — each returns its parsed result
  // directly. mem search is sync at the host; workspace ops are cheap mux ops;
  // worker/agent are job-id/poll, which is *sync per step* (each call returns a
  // ticket/status immediately — the long-running work happens between calls, on
  // the host side). They are NOT Promise-wrapped: QuickJS has no event loop to
  // drain a Promise inside a synchronous `call_json` dispatch, so wrapping would
  // break sync consumers like the chatBackend `pump`/`poll` (Item 6). Plugin code
  // that `await`s these still works — `await x` on a non-thenable yields `x`.
  host.mem = {
    search: function (opts) { return parseJson(host.memSearch(JSON.stringify(opts))) || { hits: [] }; },
  };
  host.workspace = {
    ensure: function (opts) { return parseJson(host.workspaceEnsure(JSON.stringify(opts))); },
    panes:  function (workspaceId) { return parseJson(host.workspacePanes(workspaceId)) || []; },
  };
  host.worker = {
    start: function (opts) { return parseJson(host.workerStart(JSON.stringify(opts))); },
    poll:  function (jobId) { return parseJson(host.workerPoll(jobId)); },
  };
  host.progress = function (opts) { return host.progressRaw(JSON.stringify(opts || {})); };
  host.agent = {
    spawn:  function (workspaceId, opts) { return parseJson(host.agentSpawn(workspaceId, JSON.stringify(opts))); },
    resume: function (workspaceId, sessionId) { return parseJson(host.agentResume(workspaceId, sessionId)); },
    get:    function (workspaceId, key) { return parseJson(host.agentGet(workspaceId, key)); },
    send:   function (sessionId, text) { return parseJson(host.agentSend(sessionId, text)); },
    poll:   function (ticket) { return parseJson(host.agentPoll(ticket)); },
    reap:   function (sessionId) { host.agentReap(sessionId); },
    usage:  function (sessionId) { return parseJson(host.agentUsage(sessionId)); },
  };
  host.fetch.stream = function (opts) { return parseJson(host.fetchStream(JSON.stringify(opts))); };
  host.fetch.streamPoll = function (jobId) { return parseJson(host.fetchStreamPoll(jobId)); };
  host.fetch.streamClose = function (jobId) { host.fetchStreamClose(jobId); };
  // Async exec: job-id + poll, so a slow tool call never holds the VM lock for
  // its whole duration (sibling panes share this VM). Mirrors fetch.stream.
  host.exec.start = function (opts) { return parseJson(host.execStart(JSON.stringify(opts))); };
  host.exec.poll = function (jobId) { return parseJson(host.execPoll(jobId)); };
  host.exec.close = function (jobId) { host.execClose(jobId); };
})();
"#;

#[derive(Debug)]
pub enum VmError {
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    RuntimeInit(String),
    Script(String),
    MissingFunction(String),
    InvalidJson(String, String),
    ArgSerialize(String, serde_json::Error),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::ReadFile { path, source } => {
                write!(f, "failed to read plugin file {path}: {source}")
            }
            VmError::RuntimeInit(msg) => write!(f, "failed to initialise JS runtime: {msg}"),
            VmError::Script(msg) => write!(f, "plugin script error: {msg}"),
            VmError::MissingFunction(name) => {
                write!(f, "plugin missing exported function `{name}`")
            }
            VmError::InvalidJson(name, msg) => {
                write!(f, "plugin function `{name}` returned invalid JSON: {msg}")
            }
            VmError::ArgSerialize(name, err) => {
                write!(f, "argument serialization failed for `{name}`: {err}")
            }
        }
    }
}

impl std::error::Error for VmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VmError::ReadFile { source, .. } => Some(source),
            VmError::ArgSerialize(_, err) => Some(err),
            _ => None,
        }
    }
}

/// One plugin = one VM. Holds the QuickJS runtime + context behind a
/// mutex so cross-thread callers serialize cleanly. Cheap to clone via
/// `Arc<PluginVm>`.
pub struct PluginVm {
    inner: Mutex<Inner>,
    plugin_id: String,
}

struct Inner {
    // Order matters: context must be dropped before runtime.
    _ctx: Context,
    _rt: Runtime,
    // We re-create a fresh context per call would be expensive; instead the bundle's exports live as globals (`__plugin__.detectFromTitle`, ...)
    // after the initial load. `call` reads them on demand.
    bundle_source: String,
    // Manifest JSON string returned to the plugin via `host.manifest()`. Preserved across reload so the post-reload plugin still sees its declared paths.
    manifest_json: Option<String>,
    // FS/env policy for this plugin's host calls. Preserved across reload.
    sandbox: std::sync::Arc<super::host_api::PluginSandbox>,
}

impl PluginVm {
    /// Load a plugin bundle (`plugin.js`) into a fresh QuickJS context.
    ///
    /// The bundle is expected to be a self-contained ES module that
    /// assigns its default export's fields to a `globalThis.__plugin__`
    /// object. The loader wraps the bundle's source with a tiny
    /// preamble that does this, so plugin authors don't need to know
    /// about it.
    pub fn load(plugin_id: &str, plugin_js_path: &Path) -> Result<Self, VmError> {
        let source = std::fs::read_to_string(plugin_js_path).map_err(|e| VmError::ReadFile {
            path: plugin_js_path.display().to_string(),
            source: e,
        })?;
        Self::from_source(plugin_id, source)
    }

    /// Like `load_with_manifest` but with an explicit sandbox policy. Used by
    /// the loader, which knows whether the plugin is bundled (unrestricted) or
    /// a user drop-in (confined).
    pub(super) fn load_with_manifest_sandboxed(
        plugin_id: &str,
        plugin_js_path: &Path,
        manifest_json: String,
        sandbox: std::sync::Arc<super::host_api::PluginSandbox>,
    ) -> Result<Self, VmError> {
        let source = std::fs::read_to_string(plugin_js_path).map_err(|e| VmError::ReadFile {
            path: plugin_js_path.display().to_string(),
            source: e,
        })?;
        Self::from_source_sandboxed(plugin_id, source, Some(manifest_json), sandbox)
    }

    /// Same as `load` but with the manifest JSON also installed so the
    /// plugin can read its own declared paths via `host.manifest()`.
    pub fn load_with_manifest(
        plugin_id: &str,
        plugin_js_path: &Path,
        manifest_json: String,
    ) -> Result<Self, VmError> {
        let source = std::fs::read_to_string(plugin_js_path).map_err(|e| VmError::ReadFile {
            path: plugin_js_path.display().to_string(),
            source: e,
        })?;
        Self::from_source_with(plugin_id, source, Some(manifest_json))
    }

    /// Same as `load` but takes the source string directly — used in
    /// tests and by the in-repo bundled plugin loader. Unrestricted host access.
    pub fn from_source(plugin_id: &str, bundle_source: String) -> Result<Self, VmError> {
        Self::from_source_with(plugin_id, bundle_source, None)
    }

    /// Same as `from_source`, plus a manifest JSON string the plugin can
    /// read back at runtime through `host.manifest()`. Lets plugin
    /// functions resolve their own declared paths (rule.path, sessions.rootDir,
    /// etc.) without us threading those through every call site.
    /// Unrestricted host access (tests, bundled in-repo loader). For a
    /// confined drop-in plugin, use `from_source_sandboxed`.
    pub fn from_source_with(
        plugin_id: &str,
        bundle_source: String,
        manifest_json: Option<String>,
    ) -> Result<Self, VmError> {
        Self::from_source_sandboxed(
            plugin_id,
            bundle_source,
            manifest_json,
            std::sync::Arc::new(super::host_api::PluginSandbox::unrestricted()),
        )
    }

    pub(super) fn from_source_sandboxed(
        plugin_id: &str,
        bundle_source: String,
        manifest_json: Option<String>,
        sandbox: std::sync::Arc<super::host_api::PluginSandbox>,
    ) -> Result<Self, VmError> {
        let rt = Runtime::new().map_err(|e| VmError::RuntimeInit(e.to_string()))?;
        let ctx = Context::full(&rt).map_err(|e| VmError::RuntimeInit(e.to_string()))?;

        // Normalize plugin exports to `globalThis.__plugin__` for both cjs and esm bundles.
        // esbuild builds a default-export object; we unwrap it through a tiny bootstrap IIFE.
        let bootstrap = format!(
            r#"
            globalThis.__plugin__ = (function() {{
                const module = {{ exports: {{}} }};
                const exports = module.exports;
                {source}
                // ESM default export styles esbuild may emit:
                if (module.exports.default) return module.exports.default;
                if (module.exports.__esModule && module.exports.default) return module.exports.default;
                return module.exports;
            }})();
            "#,
            source = bundle_source
        );

        let manifest_for_install = manifest_json.clone();
        let sandbox_for_install = sandbox.clone();
        ctx.with(|ctx| -> Result<(), VmError> {
            super::host_api::install(&ctx, manifest_for_install.as_deref(), sandbox_for_install)?;
            ctx.eval::<(), _>(PLUGIN_PREAMBLE)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("preamble: {e:?}")))?;
            ctx.eval::<(), _>(bootstrap.clone())
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            Ok(())
        })?;

        Ok(Self {
            inner: Mutex::new(Inner {
                _ctx: ctx,
                _rt: rt,
                bundle_source,
                manifest_json,
                sandbox,
            }),
            plugin_id: plugin_id.to_string(),
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Late-bind the host `ServiceContext` into this plugin's sandbox. The VM is
    /// constructed before `ServiceContext` exists (plan §1.5 F4), so the host
    /// wires this in once the service is built. First write wins; returns
    /// whether this call populated it.
    pub fn set_service(&self, svc: std::sync::Weak<crate::service::ServiceContext>) -> bool {
        self.inner.lock().sandbox.set_service(svc)
    }

    /// Whether this plugin holds the `agent_commands` grant (read from the
    /// sandbox — the enforcement source of truth). Drives registration of the
    /// inverse-seam handler.
    pub fn agent_commands_allowed(&self) -> bool {
        self.inner.lock().sandbox.agent_commands_allowed()
    }

    /// Re-stamp the plugin's user settings into the EXISTING sandbox without
    /// rebuilding the VM. The hot-reload watcher calls this on a config-only
    /// change so `host.settingsJson()` reflects the edit on its next call while
    /// the plugin's in-memory state (open chatBackend sessions) survives. Mirrors
    /// the `set_service` late-bind: mutate the shared sandbox in place.
    pub fn reload_settings(&self, settings_json: String) {
        self.inner.lock().sandbox.set_settings(settings_json);
    }

    /// Re-load the plugin from a fresh source string. Used by the
    /// hot-reload path when `plugin.js` changes on disk.
    pub fn reload(&self, new_source: String) -> Result<(), VmError> {
        let mut inner = self.inner.lock();
        let rt = Runtime::new().map_err(|e| VmError::RuntimeInit(e.to_string()))?;
        let ctx = Context::full(&rt).map_err(|e| VmError::RuntimeInit(e.to_string()))?;

        let bootstrap = format!(
            r#"
            globalThis.__plugin__ = (function() {{
                const module = {{ exports: {{}} }};
                const exports = module.exports;
                {source}
                if (module.exports.default) return module.exports.default;
                if (module.exports.__esModule && module.exports.default) return module.exports.default;
                return module.exports;
            }})();
            "#,
            source = new_source
        );
        let manifest_for_install = inner.manifest_json.clone();
        let sandbox_for_install = inner.sandbox.clone();
        ctx.with(|ctx| -> Result<(), VmError> {
            super::host_api::install(&ctx, manifest_for_install.as_deref(), sandbox_for_install)?;
            ctx.eval::<(), _>(PLUGIN_PREAMBLE)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("preamble: {e:?}")))?;
            ctx.eval::<(), _>(bootstrap)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            Ok(())
        })?;
        inner._ctx = ctx;
        inner._rt = rt;
        inner.bundle_source = new_source;
        Ok(())
    }
}

impl Drop for PluginVm {
    fn drop(&mut self) {
        super::host_net::drop_plugin_jobs(&self.plugin_id);
        super::host_exec::drop_plugin_jobs(&self.plugin_id);
    }
}

/// Call a method on the plugin's default export.
///
/// Arguments are serialized as JSON strings on the Rust side, parsed
/// inside the JS VM, passed positionally to the plugin's function. The
/// return value is JSON-serialized inside the VM and deserialized on
/// the Rust side. Plugins that need to return `undefined` should return
/// `null` — `undefined` becomes `null` over the JSON bridge.
///
/// This lives as a free function (not a method) so callers can pass an
/// `Arc<PluginVm>` cheaply and not borrow the whole vm for the duration.
pub fn call_json<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &str,
    args: &[serde_json::Value],
) -> Result<R, VmError> {
    // Untrusted code runs below this point. Contain a plugin-side panic so one
    // bad drop-in can't unwind through the host and take the daemon down (#16).
    // (A native stack overflow is NOT catchable here — that class is prevented
    // upstream in the registry/loader, not rescued by catch_unwind.)
    let run = std::panic::AssertUnwindSafe(|| call_json_inner::<R>(vm, method, args));
    match std::panic::catch_unwind(run) {
        Ok(res) => res,
        Err(_) => Err(VmError::Script(format!(
            "{method}: plugin panicked (caught — provider isolated)"
        ))),
    }
}

fn call_json_inner<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &str,
    args: &[serde_json::Value],
) -> Result<R, VmError> {
    let inner = vm.inner.lock();
    let result_json: String = inner._ctx.with(|ctx| -> Result<String, VmError> {
        // Pull the function off __plugin__
        let plugin_obj: Value = ctx
            .globals()
            .get("__plugin__")
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("__plugin__ not present: {e:?}")))?;
        let plugin_obj = plugin_obj
            .as_object()
            .ok_or_else(|| VmError::Script("__plugin__ is not an object".into()))?;
        let _func: Function = plugin_obj
            .get(method)
            .catch(&ctx)
            .map_err(|_| VmError::MissingFunction(method.to_string()))?;

        // Encode args as a single JSON string, parse inside the VM, spread into the call. This avoids per-arg Value construction and keeps the bridge boundary simple.
        let args_json = serde_json::to_string(args)
            .map_err(|e| VmError::ArgSerialize(method.to_string(), e))?;
        ctx.globals()
            .set("__plugin_args__", args_json)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("setting args failed: {e:?}")))?;

        // Run a tiny helper that parses args + calls + serializes result.
        let helper = format!(
            r#"
            (function() {{
                const args = JSON.parse(globalThis.__plugin_args__);
                const fn = globalThis.__plugin__[{name:?}];
                if (typeof fn !== 'function') {{
                    throw new Error('not a function: ' + {name:?});
                }}
                const out = fn.apply(globalThis.__plugin__, args);
                // Promise returns aren't supported here — providers declare sync methods only.
                if (out && typeof out.then === 'function') {{
                    throw new Error({name:?} + ' returned a Promise; plugin methods must be synchronous');
                }}
                return JSON.stringify(out === undefined ? null : out);
            }})()
            "#,
            name = method,
        );
        let result: String = ctx
            .eval(helper)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("{method}: {e:?}")))?;
        Ok(result)
    })?;
    // Drop the lock before parsing.
    drop(inner);
    serde_json::from_str(&result_json)
        .map_err(|e| VmError::InvalidJson(method.to_string(), e.to_string()))
}

/// Whether the plugin exports `method` as a function. Lets the host tell
/// "plugin doesn't implement this capability" (→ delegate to a native
/// fallback) apart from "plugin implements it and returned null/empty".
pub fn has_method(vm: &PluginVm, method: &str) -> bool {
    let inner = vm.inner.lock();
    inner
        ._ctx
        .with(|ctx| -> Result<String, VmError> {
            let expr = format!(
                "(typeof globalThis.__plugin__ === 'object' && globalThis.__plugin__ !== null \
                 && typeof globalThis.__plugin__[{method:?}] === 'function') ? '1' : '0'"
            );
            ctx.eval(expr)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))
        })
        .map(|s| s == "1")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(src: &str) -> PluginVm {
        PluginVm::from_source("test", src.to_string()).expect("load")
    }

    #[test]
    fn round_trip_string_arg_and_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                upper(s) { return s.toUpperCase(); }
            };
        "#,
        );
        let out: String = call_json(&vm, "upper", &[json!("hi")]).unwrap();
        assert_eq!(out, "HI");
    }

    #[test]
    fn round_trip_object_arg_and_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                meta() { return { id: "x", displayName: "X" }; }
            };
        "#,
        );
        let out: serde_json::Value = call_json(&vm, "meta", &[]).unwrap();
        assert_eq!(out["id"], "x");
        assert_eq!(out["displayName"], "X");
    }

    #[test]
    fn round_trip_array_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                things() { return [1, 2, 3]; }
            };
        "#,
        );
        let out: Vec<i32> = call_json(&vm, "things", &[]).unwrap();
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn missing_function_errors() {
        let vm = fixture("module.exports.default = {};");
        let err = call_json::<()>(&vm, "nope", &[]).unwrap_err();
        match err {
            VmError::Script(_) | VmError::MissingFunction(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn plugin_throw_surfaces_as_script_error() {
        let vm = fixture(
            r#"
            module.exports.default = {
                kaboom() { throw new Error('nope'); }
            };
        "#,
        );
        let err = call_json::<()>(&vm, "kaboom", &[]).unwrap_err();
        assert!(matches!(err, VmError::Script(_)));
    }

    #[test]
    fn returning_undefined_becomes_null_in_rust() {
        let vm = fixture(
            r#"
            module.exports.default = {
                nothing() { /* implicit undefined */ }
            };
        "#,
        );
        let out: Option<i32> = call_json(&vm, "nothing", &[]).unwrap();
        assert_eq!(out, None);
    }

    #[test]
    fn promise_return_rejected() {
        let vm = fixture(
            r#"
            module.exports.default = {
                async slow() { return 1; }
            };
        "#,
        );
        let err = call_json::<i32>(&vm, "slow", &[]).unwrap_err();
        assert!(matches!(err, VmError::Script(_)));
    }

    #[test]
    fn boolean_return() {
        let vm = fixture(
            r#"
            module.exports.default = {
                yes() { return true; },
                no()  { return false; }
            };
        "#,
        );
        let yes: bool = call_json(&vm, "yes", &[]).unwrap();
        let no: bool = call_json(&vm, "no", &[]).unwrap();
        assert!(yes);
        assert!(!no);
    }

    #[test]
    fn esm_export_default_style() {
        // esbuild output for `export default { ... }` produces `module.exports.default = ...` after the bootstrap interpret.
        let vm = fixture(
            r#"
            // Pretend this is esbuild output for `export default { ... }`
            Object.defineProperty(module.exports, "__esModule", { value: true });
            module.exports.default = {
                hello() { return "world"; }
            };
        "#,
        );
        let out: String = call_json(&vm, "hello", &[]).unwrap();
        assert_eq!(out, "world");
    }

    #[test]
    fn preamble_exposes_host_fs_and_host_manifest() {
        // Plugin uses ONLY the convenience surface — no JSON.parse, no raw *Json suffix calls. If the preamble didn't load, host.fs / host.manifest
        // would be undefined and these calls would throw TypeError.
        let vm = PluginVm::from_source_with(
            "test",
            r#"
            module.exports.default = {
                listNames(p) { return host.fs.readDir(p).map(e => e.name).sort(); },
                getManifestId() {
                    var m = host.manifest();
                    return m ? m.id : null;
                },
                wrap(s) { return host.shell.quote(s); }
            };
            "#
            .to_string(),
            Some(r#"{"id":"X","version":"0","displayName":"X","spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}}"#.to_string()),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), "1").unwrap();
        std::fs::write(dir.path().join("b"), "2").unwrap();
        let names: Vec<String> =
            call_json(&vm, "listNames", &[json!(dir.path().to_string_lossy())]).unwrap();
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
        let id: Option<String> = call_json(&vm, "getManifestId", &[]).unwrap();
        assert_eq!(id.as_deref(), Some("X"));
        let q: String = call_json(&vm, "wrap", &[json!("can't")]).unwrap();
        assert_eq!(q, "'can'\\''t'");
    }

    #[test]
    fn preamble_exposes_host_toolcall_parse() {
        let vm = fixture(
            r#"
            module.exports.default = {
                parse(raw, schema) { return host.toolcall.parse(raw, schema); }
            };
            "#,
        );
        let schema = r#"{
            "tools":[{"name":"exec","args":["cmd"]}],
            "aliases":{"command":"cmd"}
        }"#;
        let raw = r#"```codeterm-tool
{"tool":"exec","args":{"command":"ls"}}
```"#;

        let via_host: String = call_json(&vm, "parse", &[json!(raw), json!(schema)]).unwrap();
        let native = super::super::toolcall_parse::parse_to_json(raw, schema);
        assert_eq!(via_host, native);
        let parsed: serde_json::Value = serde_json::from_str(&via_host).unwrap();
        assert_eq!(parsed["tool"], "exec");
        assert_eq!(parsed["args"]["cmd"], "ls");
    }

    #[test]
    fn preamble_compute_is_fully_synchronous() {
        // Item 6: ALL compute binds (mem/workspace/worker/agent) are synchronous
        // — each returns its parsed result DIRECTLY, never a Promise. A Promise
        // would be undrainable inside a sync `call_json` dispatch and would break
        // the chatBackend `pump`/`poll` consumers. No service is bound in
        // from_source, so the underlying sync binds return the `{ error }`
        // envelope — we assert the SHAPE (sync, not thenable) here.
        let vm = fixture(
            r#"
            module.exports.default = {
                shape() {
                    var mem = host.mem.search({ query: "q" });
                    var start = host.worker.start({ task: "t" });
                    var send = host.agent.send("s", "t");
                    return {
                        memIsFn:       typeof host.mem.search === "function",
                        wsEnsureIsFn:  typeof host.workspace.ensure === "function",
                        wsPanesIsFn:   typeof host.workspace.panes === "function",
                        // every result is a plain object, NOT a thenable.
                        memSync:       mem !== null && typeof mem === "object" && mem.then === undefined,
                        memError:      typeof mem.error === "string",
                        workerSync:    start !== null && typeof start === "object" && start.then === undefined,
                        agentSync:     send !== null && typeof send === "object" && send.then === undefined,
                    };
                }
            };
            "#,
        );
        let v: serde_json::Value = call_json(&vm, "shape", &[]).unwrap();
        assert_eq!(v["memIsFn"], true);
        assert_eq!(v["wsEnsureIsFn"], true);
        assert_eq!(v["wsPanesIsFn"], true);
        assert_eq!(v["memSync"], true, "host.mem.search must be synchronous");
        assert_eq!(
            v["memError"], true,
            "svc-absent must surface an error envelope"
        );
        assert_eq!(
            v["workerSync"], true,
            "host.worker.start must be synchronous (no Promise wrapper)"
        );
        assert_eq!(
            v["agentSync"], true,
            "host.agent.send must be synchronous (no Promise wrapper)"
        );
    }

    #[test]
    fn host_compute_perms_gate_workspace_and_agent() {
        // §1.10 P3: host.workspace* requires manage_workspaces; host.agent*
        // requires spawn_agents. A restricted sandbox without the grant is
        // denied at the bind; with the grant it passes the gate (svc absent in a
        // unit test → "host service unavailable", proving the gate was crossed).
        use crate::plugin::js_runtime::host_api::PluginSandbox;
        use crate::plugin::js_runtime::manifest::PermissionsConfig;
        use std::sync::Arc;
        const SRC: &str = r#"
            module.exports.default = {
                ws() { return host.workspaceEnsure("{}"); },
                ag() { return host.agentSpawn("w", "{}"); }
            };
        "#;

        let denied = Arc::new(
            PluginSandbox::restricted(vec![], vec![])
                .with_identity_and_perms("p", &PermissionsConfig::default()),
        );
        let vm = PluginVm::from_source_sandboxed("p", SRC.to_string(), None, denied).unwrap();
        let ws: String = call_json(&vm, "ws", &[]).unwrap();
        assert!(ws.contains("manage_workspaces permission required"), "{ws}");
        let ag: String = call_json(&vm, "ag", &[]).unwrap();
        assert!(ag.contains("spawn_agents permission required"), "{ag}");

        let perms = PermissionsConfig {
            manage_workspaces: true,
            spawn_agents: true,
            ..Default::default()
        };
        let granted = Arc::new(
            PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms("p", &perms),
        );
        let vm2 = PluginVm::from_source_sandboxed("p", SRC.to_string(), None, granted).unwrap();
        let ws2: String = call_json(&vm2, "ws", &[]).unwrap();
        assert!(
            ws2.contains("host service unavailable"),
            "granted ws: {ws2}"
        );
        let ag2: String = call_json(&vm2, "ag", &[]).unwrap();
        assert!(
            ag2.contains("host service unavailable"),
            "granted ag: {ag2}"
        );
    }

    #[test]
    fn fetch_stream_preamble_is_sync_and_uses_fetch_allowlist() {
        use crate::plugin::js_runtime::host_api::PluginSandbox;
        use crate::plugin::js_runtime::manifest::PermissionsConfig;
        use std::sync::Arc;

        const SRC: &str = r#"
            module.exports.default = {
                shape() {
                    var result = host.fetch.stream({ url: "http://blocked.example.test/stream" });
                    return {
                        streamIsFn: typeof host.fetch.stream === "function",
                        pollIsFn: typeof host.fetch.streamPoll === "function",
                        closeIsFn: typeof host.fetch.streamClose === "function",
                        sync: !(result && typeof result.then === "function"),
                        error: result && result.error,
                    };
                }
            };
        "#;

        let sandbox = Arc::new(
            PluginSandbox::restricted(vec![], vec![])
                .with_identity_and_perms("p", &PermissionsConfig::default()),
        );
        let vm = PluginVm::from_source_sandboxed("p", SRC.to_string(), None, sandbox).unwrap();
        let shape: serde_json::Value = call_json(&vm, "shape", &[]).unwrap();
        assert_eq!(shape["streamIsFn"], true);
        assert_eq!(shape["pollIsFn"], true);
        assert_eq!(shape["closeIsFn"], true);
        assert_eq!(shape["sync"], true);
        assert_eq!(
            shape["error"], "fetch denied: blocked.example.test",
            "stream must share host.fetch's network allowlist denial"
        );
    }

    #[test]
    fn reload_swaps_implementation() {
        let vm = fixture(
            r#"
            module.exports.default = { v() { return 1; } };
        "#,
        );
        let v1: i32 = call_json(&vm, "v", &[]).unwrap();
        assert_eq!(v1, 1);

        vm.reload(
            r#"
            module.exports.default = { v() { return 2; } };
        "#
            .to_string(),
        )
        .unwrap();
        let v2: i32 = call_json(&vm, "v", &[]).unwrap();
        assert_eq!(v2, 2);
    }
}
