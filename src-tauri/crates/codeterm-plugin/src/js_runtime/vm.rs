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
    readFileHead:  host.readFileHead,
    readFileTail:  host.readFileTail,
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
    quote:     host.shellQuote,
    quoteFor:  host.shellQuoteFor,
    envPrefix: host.shellEnvPrefix,
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
  // null = denied/unreadable; [] = readable but no child dirs.
  host.listChildDirs = function (p) { return parseJson(host.listChildDirsJson(p)); };

  // sync per step). Not Promise-wrapped: QuickJS can't drain a Promise in a sync `call_json` dispatch.
  host.mem = {
    search: function (opts) { return parseJson(host.memSearch(JSON.stringify(opts))) || { hits: [] }; },
  };
  host.workspace = {
    ensure: function (opts) { return parseJson(host.workspaceEnsure(JSON.stringify(opts))); },
    tabs: function (workspaceId) { return parseJson(host.workspaceTabs(workspaceId)) || []; },
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
  // Async exec (job-id + poll) so a slow tool call never holds the shared VM lock for its whole duration.
  host.exec.start = function (opts) { return parseJson(host.execStart(JSON.stringify(opts))); };
  host.exec.poll = function (jobId) { return parseJson(host.execPoll(jobId)); };
  host.exec.close = function (jobId) { host.execClose(jobId); };
  // Return `host.awaitJob(job, then)` from a plugin method instead of blocking in
  // it: the frame unwinds, the host waits for the job with the VM released, and
  var __awaitSeq__ = 0;
  var __awaits__ = {};
  globalThis.__ct_await_take__ = function (k) {
    var f = __awaits__[k];
    delete __awaits__[k];
    return f;
  };
  host.awaitJob = function (jobId, then) {
    if (typeof then !== "function") { throw new Error("host.awaitJob requires a continuation function"); }
    if (typeof jobId !== "string" || jobId === "") { return then({ error: "host.awaitJob: missing job id" }); }
    var k = String(++__awaitSeq__);
    __awaits__[k] = then;
    return { __ctAwait__: { job: jobId, k: k } };
  };
  host.exec.async = function (opts, then) {
    var j = host.exec.start(opts);
    if (!j || typeof j.jobId !== "string") { return then({ error: (j && j.error) || "exec.start failed" }); }
    return host.awaitJob(j.jobId, then);
  };
})();
"#;

const INVOKE_HELPERS: &str = r#"
globalThis.__ct_invoke__ = function () {
    var m = globalThis.__ct_method__;
    var p = globalThis.__plugin__;
    var fn = p ? p[m] : undefined;
    if (typeof fn !== 'function') { throw new Error('not a function: ' + m); }
    var out = fn.apply(p, JSON.parse(globalThis.__ct_args__));
    if (out && typeof out.then === 'function') {
        throw new Error(m + ' returned a Promise; plugin methods must be synchronous');
    }
    return JSON.stringify(out === undefined ? null : out);
};
globalThis.__ct_resume__ = function () {
    var take = globalThis.__ct_await_take__;
    var fn = typeof take === 'function' ? take(globalThis.__ct_await_k__) : undefined;
    if (typeof fn !== 'function') { throw new Error('await continuation is gone'); }
    var out = fn(JSON.parse(globalThis.__ct_await_result__));
    if (out && typeof out.then === 'function') {
        throw new Error('await continuation returned a Promise; plugin methods must be synchronous');
    }
    return JSON.stringify(out === undefined ? null : out);
};
globalThis.__ct_has__ = function () {
    var p = globalThis.__plugin__;
    return (typeof p === 'object' && p !== null
        && typeof p[globalThis.__ct_method__] === 'function') ? '1' : '0';
};
"#;

use std::cell::Cell;

thread_local! {
    static SOURCE_COMPILES: Cell<u64> = const { Cell::new(0) };
    /// Set when the in-flight call released the VM to wait for a host job, so
    /// the slow-call warning can still mean "held the context this long".
    static CALL_YIELDED: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn source_compiles() -> u64 {
    SOURCE_COMPILES.with(|c| c.get())
}

// single source of truth for "did we recompile". Cached-Function calls bypass it.
fn eval_source<'js, V, S>(ctx: &rquickjs::Ctx<'js>, src: S) -> rquickjs::Result<V>
where
    V: rquickjs::FromJs<'js>,
    S: Into<Vec<u8>>,
{
    SOURCE_COMPILES.with(|c| c.set(c.get() + 1));
    ctx.eval(src)
}

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
    AsyncJobTimedOut,
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
            VmError::AsyncJobTimedOut => write!(f, "{ASYNC_JOB_TIMEOUT_ERROR}"),
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

/// Stable refusal returned when an async host job misses the VM call bound.
pub const ASYNC_JOB_TIMEOUT_ERROR: &str = "plugin async job exceeded its bounded wait";

/// One plugin = one VM. Holds the QuickJS runtime + context behind a
/// mutex so cross-thread callers serialize cleanly. Cheap to clone via
/// `Arc<PluginVm>`.
pub struct PluginVm {
    inner: Mutex<Inner>,
    /// Outside `inner` on purpose. Nothing here needs the context, so reading it
    /// must not queue behind a slow call; and `host.exec` jobs are keyed by this
    /// identity, so awaiting one must not read it through the lock it releases.
    sandbox: std::sync::Arc<super::host_api::PluginSandbox>,
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
            std::sync::Arc::new(
                super::host_api::PluginSandbox::unrestricted().with_identity(plugin_id),
            ),
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
            eval_source::<(), _>(&ctx, PLUGIN_PREAMBLE)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("preamble: {e:?}")))?;
            eval_source::<(), _>(&ctx, bootstrap.clone())
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            eval_source::<(), _>(&ctx, INVOKE_HELPERS)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("invoke-helpers: {e:?}")))?;
            Ok(())
        })?;

        Ok(Self {
            inner: Mutex::new(Inner {
                _ctx: ctx,
                _rt: rt,
                bundle_source,
                manifest_json,
            }),
            sandbox,
            plugin_id: plugin_id.to_string(),
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Late-bind the host `ServiceContext` (the VM is built before it exists). First write wins.
    pub fn set_service(&self, svc: std::sync::Weak<dyn crate::host_traits::PluginHostCtx>) -> bool {
        self.sandbox.set_service(svc)
    }

    /// Resolve the live host context, if one has been bound.
    pub fn service(&self) -> Option<std::sync::Arc<dyn crate::host_traits::PluginHostCtx>> {
        self.sandbox.service()
    }

    /// Whether this plugin holds the `agent_commands` grant — drives inverse-seam handler registration.
    pub fn agent_commands_allowed(&self) -> bool {
        self.sandbox.agent_commands_allowed()
    }

    /// How many sandbox capabilities this plugin has been denied. Snapshot it
    /// around a plugin call: a rise means the call failed because the plugin is
    /// confined, which is a state a user has to be told about rather than a
    /// gauge that quietly stops moving.
    pub fn capability_denial_count(&self) -> u64 {
        self.sandbox.denial_count()
    }

    /// Record a host-supplied tab cwd as a sandbox read-root (no-op when unrestricted).
    pub fn grant_cwd_read_root(&self, cwd: &str) {
        self.sandbox.grant_cwd_read_root(cwd);
    }

    /// Re-stamp the plugin's settings into the existing sandbox without rebuilding the
    /// VM, so a config-only reload preserves in-memory state (open chatBackend sessions).
    pub fn reload_settings(&self, settings_json: String) {
        self.sandbox.set_settings(settings_json);
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
        let sandbox_for_install = self.sandbox.clone();
        ctx.with(|ctx| -> Result<(), VmError> {
            super::host_api::install(&ctx, manifest_for_install.as_deref(), sandbox_for_install)?;
            eval_source::<(), _>(&ctx, PLUGIN_PREAMBLE)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("preamble: {e:?}")))?;
            eval_source::<(), _>(&ctx, bootstrap)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            eval_source::<(), _>(&ctx, INVOKE_HELPERS)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("invoke-helpers: {e:?}")))?;
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
        // The sandbox's id, not the VM's: that is what `host.exec`/`host.fetch`
        // keyed the jobs by when the plugin started them.
        super::host_net::drop_plugin_jobs(self.sandbox.plugin_id());
        super::host_exec::drop_plugin_jobs(self.sandbox.plugin_id());
    }
}

/// Record a host-composed `ctx.cwd` as a read-root grant on the plugin's
/// sandbox. Callers must only pass ctx objects the host itself assembled.
pub(super) fn grant_ctx_cwd(vm: &PluginVm, ctx: &serde_json::Value) {
    if let Some(cwd) = ctx.get("cwd").and_then(|v| v.as_str()) {
        vm.grant_cwd_read_root(cwd);
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
/// A call past this is worth a line: the VM mutex is held for the whole call, so
/// a plugin that blocks in a synchronous `host.exec` is not merely slow for its
/// own caller — every other call into that plugin waits behind it. Reported in
/// release builds too, because that is where a user meets it (#319, #4). A call
/// that yielded via `host.awaitJob` did not hold the VM and is not reported.
const SLOW_CALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(1);

pub fn call_json<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &str,
    args: &[serde_json::Value],
) -> Result<R, VmError> {
    let started = std::time::Instant::now();
    CALL_YIELDED.with(|y| y.set(false));
    // bad drop-in can't unwind through the host and take the daemon down (#16).
    let run = std::panic::AssertUnwindSafe(|| call_json_inner::<R>(vm, method, args));
    let result = match std::panic::catch_unwind(run) {
        Ok(res) => res,
        Err(_) => Err(VmError::Script(format!(
            "{method}: plugin panicked (caught — provider isolated)"
        ))),
    };
    let held = started.elapsed();
    if held >= SLOW_CALL_THRESHOLD && !CALL_YIELDED.with(|y| y.get()) {
        log::warn!(
            "[plugin-vm] {}.{} held the VM for {}ms — other calls into this plugin queued behind it",
            vm.plugin_id,
            method,
            held.as_millis()
        );
    }
    result
}

/// A method that returned `host.awaitJob(job, then)` instead of a value. Matched by
/// exact shape (sole key, two string fields) so an ordinary object return is
/// never mistaken for one.
struct AwaitRequest {
    job: String,
    continuation: String,
}

const AWAIT_MARKER_PREFIX: &str = "{\"__ctAwait__\":";

fn parse_await(result_json: &str) -> Option<AwaitRequest> {
    if !result_json.starts_with(AWAIT_MARKER_PREFIX) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(result_json).ok()?;
    let outer = value.as_object()?;
    if outer.len() != 1 {
        return None;
    }
    let marker = outer.get("__ctAwait__")?.as_object()?;
    if marker.len() != 2 {
        return None;
    }
    Some(AwaitRequest {
        job: marker.get("job")?.as_str()?.to_string(),
        continuation: marker.get("k")?.as_str()?.to_string(),
    })
}

fn call_json_inner<R: serde::de::DeserializeOwned>(
    vm: &PluginVm,
    method: &str,
    args: &[serde_json::Value],
) -> Result<R, VmError> {
    let args_json =
        serde_json::to_string(args).map_err(|e| VmError::ArgSerialize(method.to_string(), e))?;
    let mut result_json = enter_call(vm, method, &args_json)?;
    while let Some(pending) = parse_await(&result_json) {
        CALL_YIELDED.with(|y| y.set(true));
        // No guard is held here: the frame unwound to produce the marker, so the
        // context is idle and this wait costs no other caller.
        let job_result = match super::host_exec::wait_json(vm.sandbox.plugin_id(), &pending.job) {
            super::host_exec::WaitResult::Completed(result) => result,
            super::host_exec::WaitResult::TimedOut => return Err(VmError::AsyncJobTimedOut),
        };
        result_json = resume_call(vm, method, &pending.continuation, &job_result)?;
    }
    serde_json::from_str(&result_json)
        .map_err(|e| VmError::InvalidJson(method.to_string(), e.to_string()))
}

/// One entry into the context, guard taken and released around it. Every entry
/// into `_ctx` goes through `enter_call` or `resume_call`, which is what keeps a
/// single JS frame live at a time.
fn enter_call(vm: &PluginVm, method: &str, args_json: &str) -> Result<String, VmError> {
    let inner = vm.inner.lock();
    inner._ctx.with(|ctx| -> Result<String, VmError> {
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

        let globals = ctx.globals();
        globals
            .set("__ct_method__", method)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("setting method failed: {e:?}")))?;
        globals
            .set("__ct_args__", args_json)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("setting args failed: {e:?}")))?;
        let invoke: Function = globals
            .get("__ct_invoke__")
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("__ct_invoke__ missing: {e:?}")))?;
        let result: String = invoke
            .call(())
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("{method}: {e:?}")))?;
        Ok(result)
    })
}

fn resume_call(
    vm: &PluginVm,
    method: &str,
    continuation: &str,
    job_result: &str,
) -> Result<String, VmError> {
    let inner = vm.inner.lock();
    inner._ctx.with(|ctx| -> Result<String, VmError> {
        let globals = ctx.globals();
        globals
            .set("__ct_await_k__", continuation)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("setting await key failed: {e:?}")))?;
        globals
            .set("__ct_await_result__", job_result)
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("setting await result failed: {e:?}")))?;
        let resume: Function = globals
            .get("__ct_resume__")
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("__ct_resume__ missing: {e:?}")))?;
        let result: String = resume
            .call(())
            .catch(&ctx)
            .map_err(|e| VmError::Script(format!("{method}: {e:?}")))?;
        Ok(result)
    })
}

/// Whether the plugin exports `method` as a function. Lets the host tell
/// "plugin doesn't implement this capability" (→ delegate to a native
/// fallback) apart from "plugin implements it and returned null/empty".
pub fn has_method(vm: &PluginVm, method: &str) -> bool {
    let inner = vm.inner.lock();
    inner
        ._ctx
        .with(|ctx| -> Result<String, VmError> {
            let globals = ctx.globals();
            globals
                .set("__ct_method__", method)
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            let has: Function = globals
                .get("__ct_has__")
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))?;
            has.call(())
                .catch(&ctx)
                .map_err(|e| VmError::Script(format!("{e:?}")))
        })
        .map(|s| s == "1")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
