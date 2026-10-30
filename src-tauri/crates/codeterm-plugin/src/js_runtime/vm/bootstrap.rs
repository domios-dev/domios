/// Convenience layer wrapped around the raw `host` API. Loaded into
/// every plugin VM right after `host_api::install` and before the
/// plugin's own code, so plugin authors get parsed objects out of
/// readDir/walkDir/sqliteQuery/manifest without writing JSON.parse
/// boilerplate at every call site.
///
/// Keep `host.readDirJson` / `host.walkDirJson` / `host.sqliteQueryAllJson`
/// / `host.manifestJson` reachable as raw escape hatches; the preamble
/// only **adds** properties, never shadows the raw ones.
pub(super) const PLUGIN_PREAMBLE: &str = r#"
(function() {
  if (typeof host !== "object" || host === null) return;
  function parseJson(s) {
    if (s === null || s === undefined) return null;
    try { return JSON.parse(s); } catch (_) { return null; }
  }
  // The SDK declares readFile as string | null; the native binding yields undefined.
  var nativeReadFile = host.readFile;
  host.readFile = function (p) { var s = nativeReadFile(p); return s === undefined ? null : s; };
  host.fs = {
    readFile:      host.readFile,
    readFileHead:  host.readFileHead,
    readFileTail:  host.readFileTail,
    writeFile:     host.writeFile,
    writeFileAtomic: host.writeFileAtomic,
    lockFile:      host.lockFile,
    unlockFile:    host.unlockFile,
    removeFile:    host.removeFile,
    makeDirs:      host.makeDirs,
    fileExists:    host.fileExists,
    isSymlink:     host.isSymlink,
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
    // Same rows as sqliteQuery, read on a host thread while the VM serves other calls.
    sqliteQueryAsync: function (db, sql, params, then) {
      if (typeof then !== "function") { throw new Error("host.fs.sqliteQueryAsync requires a continuation function"); }
      var pj = (params === undefined || params === null) ? null : JSON.stringify(params);
      var j = parseJson(host.sqliteQueryStart(db, sql, pj));
      if (j && typeof j.jobId === "string") {
        return host.awaitJob(j.jobId, function (r) {
          if (r && typeof r.rangeError === "string") { throw new RangeError(r.rangeError); }
          return then((r && r.rows) || []);
        });
      }
      if (j && typeof j.error === "string") { return then(host.fs.sqliteQuery(db, sql, params)); }
      return then([]);
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
  // null means the host could not answer; a refusal arrives as an { error } object.
  host.analytics = {
    query: function (query) { return parseJson(host.analyticsQuery(JSON.stringify(query || {}))); },
  };
  host.events = {
    emit: function (event) { return parseJson(host.graphEventEmit(JSON.stringify(event || {}))); },
  };
  // Returns the JSON text `{ result }` or `{ error }` exactly as the host produced it.
  var surfaceDispatch = host.surfaceCall;
  var surfaceBegin = host.surfaceInvocationBegin;
  var surfaceEnd = host.surfaceInvocationEnd;
  var awaitInvocationId = host.awaitInvocationId;
  var awaitTakeMode = host.awaitTakeMode;
  delete host.awaitInvocationId;
  delete host.awaitTakeMode;
  delete host.surfaceInvocationBegin;
  delete host.surfaceInvocationEnd;
  var surfaceLogical = null;
  var surfaceSequence = 0;
  function finishSurfaceResult(value, invocation, logical) {
    if (typeof value === "string") {
      if (value.length > 32768) return JSON.stringify({error:"surface result exceeds 32768 characters"});
      return JSON.stringify({result:value});
    }
    var marker = value && typeof value === "object" && Object.keys(value).length === 1 && value.__ctAwait__;
    if (marker && typeof marker === "object" && Object.keys(marker).length === 2 && typeof marker.job === "string" && typeof marker.k === "string") {
      var entry = __awaits__[marker.k];
      if (entry && entry.job === marker.job && entry.logical === logical && entry.owner === awaitInvocationId()) {
        delete __awaits__[marker.k];
        return ctAwaitJob(marker.job, function (result) {
          return invokeSurface(invocation, logical, function () { return entry.fn(result); });
        });
      }
    }
    return JSON.stringify({error:"onSurfaceCall must return a string or its own live host continuation"});
  }
  function invokeSurface(invocation, logical, work) {
    if (invocation && !surfaceBegin(invocation)) return JSON.stringify({error:"surface invocation expired"});
    var previous = surfaceLogical;
    var suspended = false;
    surfaceLogical = logical;
    try {
      var result = finishSurfaceResult(work(), invocation, logical);
      suspended = result && typeof result === "object" && !!result.__ctAwait__;
      return result;
    } catch (_) { return JSON.stringify({error:"view surface verb failed"}); }
    finally {
      surfaceLogical = previous;
      if (invocation) surfaceEnd(invocation, suspended);
    }
  }
  host.surface = {
    call: function (verb, argsJson) {
      var args = typeof argsJson === "string" ? argsJson : JSON.stringify(argsJson === undefined ? [] : argsJson);
      var reply = surfaceDispatch(String(verb || ""), args);
      var plan = parseJson(reply);
      if (!plan || !plan.view_dispatch) return reply;
      var dispatch = plan.view_dispatch;
      return invokeSurface(dispatch.invocation, "surface-" + String(++surfaceSequence), function () {
        if (!globalThis.__plugin__ || typeof globalThis.__plugin__.onSurfaceCall !== "function")
          throw new Error("plugin declares view verbs but has no onSurfaceCall");
        return globalThis.__plugin__.onSurfaceCall(dispatch.context, dispatch.verb, dispatch.args);
      });
    },
    invalidate: function (identity) { return parseJson(host.surfaceInvalidate(JSON.stringify(identity || {}))); },
  };
  var callSurface = host.surface.call;
  function surfaceDispatchResult(value) {
    if (typeof value === "string") return parseJson(value) || {error:"invalid surface result"};
    var marker = value && value.__ctAwait__;
    var entry = marker && __awaits__[marker.k];
    if (!entry || entry.job !== marker.job || entry.owner !== awaitInvocationId()) return {error:"surface continuation expired"};
    delete __awaits__[marker.k];
    return ctAwaitJob(marker.job, function (result) { return surfaceDispatchResult(entry.fn(result)); });
  }
  host.surface.dispatch = function (verb, args) { return surfaceDispatchResult(callSurface(verb, args)); };
  host.receiverTarget = {
    get:   function () { return parseJson(host.receiverTargetGet()); },
    clear: function () { return parseJson(host.receiverTargetClear()); },
  };
  host.localRecording = {
    listInputDevices: function () { return parseJson(host.localRecordingInputs()); },
    selectDevices: function (devices) { return parseJson(host.localRecordingSelect(JSON.stringify(devices))); },
    savedSelection: function () { return parseJson(host.localRecordingSavedSelection()); },
    statusReport: function () { return parseJson(host.receiverStatusReport()); },
    status: function () { return parseJson(host.localRecordingStatus()); },
    setEnabled: function (enabled) { return parseJson(host.localRecordingSetEnabled(enabled === true)); },
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
  host.transfer = {
    start: function(opts) { return parseJson(host.transferStart(JSON.stringify(opts))); },
    abort: function(jobId) { return host.transferAbort(jobId); },
    bindInput: function(handle,destinationOrigin,then) {
      if (typeof then !== "function") throw new Error("host.transfer.bindInput requires a continuation");
      var job=parseJson(host.transferBindInput(JSON.stringify({handle:handle,destinationOrigin:destinationOrigin})));
      if (!job || typeof job.jobId!=="string") return then(job);
      return host.awaitJob(job.jobId,then);
    },
    async: function(opts, then) {
      if (typeof then !== "function") throw new Error("host.transfer.async requires a continuation");
      var job = this.start(opts);
      if (!job || typeof job.jobId !== "string") return then(job);
      return host.awaitJob(job.jobId, then);
    },
  };
  host.fetch.stream = function (opts) { return parseJson(host.fetchStream(JSON.stringify(opts))); };
  host.fetch.streamPoll = function (jobId) { return parseJson(host.fetchStreamPoll(jobId)); };
  host.fetch.streamClose = function (jobId) { host.fetchStreamClose(jobId); };
  host.fetch.async = function (opts, then) {
    if (typeof then !== "function") { throw new Error("host.fetch.async requires a continuation function"); }
    var j = parseJson(host.fetchStart(JSON.stringify(opts)));
    if (!j || typeof j.jobId !== "string") { return then({ error: (j && j.error) || "fetch start failed" }); }
    return host.awaitJob(j.jobId, then);
  };
  // Async exec (job-id + poll) so a slow tool call never holds the shared VM lock for its whole duration.
  host.exec.start = function (opts) { return parseJson(host.execStart(JSON.stringify(opts))); };
  host.exec.poll = function (jobId) { return parseJson(host.execPoll(jobId)); };
  host.exec.close = function (jobId) { host.execClose(jobId); };
  // Awaiting a job must release the VM frame so the host can complete it and re-enter only for the continuation.
  var __awaitSeq__ = 0;
  var __awaits__ = {};
  Object.defineProperty(globalThis, "__ct_await_take__", { value: function (k) {
    var entry = __awaits__[k];
    if (!entry) return undefined;
    var mode = awaitTakeMode(k, entry.job, entry.owner);
    if (!mode) return undefined;
    delete __awaits__[k];
    return mode === "resume" ? entry.fn : undefined;
  }, writable:false, configurable:false });
  Object.defineProperty(globalThis, "__ct_await_validate__", { value: function (k, job) {
    var entry = __awaits__[k];
    return !!entry && entry.job === job && entry.owner === awaitInvocationId();
  }, writable:false, configurable:false });
  Object.defineProperty(globalThis, "__ct_await_discard_root__", { value: function () {
    var owner = awaitInvocationId();
    Object.keys(__awaits__).forEach(function (key) { if (__awaits__[key].owner === owner) delete __awaits__[key]; });
  }, writable:false, configurable:false });
  function ctAwaitJob(jobId, then) {
    if (typeof then !== "function") { throw new Error("host.awaitJob requires a continuation function"); }
    if (typeof jobId !== "string" || jobId === "") { return then({ error: "host.awaitJob: missing job id" }); }
    var k = String(++__awaitSeq__);
    __awaits__[k] = {fn:then,job:jobId,logical:surfaceLogical,owner:awaitInvocationId()};
    return { __ctAwait__: { job: jobId, k: k } };
  }
  host.awaitJob = ctAwaitJob;
  host.exec.async = function (opts, then) {
    var j = host.exec.start(opts);
    if (!j || typeof j.jobId !== "string") { return then({ error: (j && j.error) || "exec.start failed" }); }
    return host.awaitJob(j.jobId, then);
  };
})();
"#;
