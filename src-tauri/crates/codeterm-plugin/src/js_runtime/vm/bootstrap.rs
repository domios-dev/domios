pub(super) const PLUGIN_PREAMBLE: &str = r#"
(function() {
  if (typeof host !== "object" || host === null) return;
  function parseJson(s) {
    if (s === null || s === undefined) return null;
    try { return JSON.parse(s); } catch (_) { return null; }
  }
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
  var awaitInvocationId = host.awaitInvocationId;
  var awaitTakeMode = host.awaitTakeMode;
  delete host.awaitInvocationId;
  delete host.awaitTakeMode;
  function agentRunAwait(job, then) {
    if (!job || typeof job.jobId !== "string") return then(job);
    return host.awaitJob(job.jobId, function(reply) { if (reply) delete reply.done; return then(reply); });
  }
  host.agentRun = {
    start: function(opts, then) {
      if (typeof then !== "function") throw new Error("agentRun.start requires a continuation");
      var job = parseJson(host.agentRunStart(JSON.stringify(opts)));
      return agentRunAwait(job, then);
    },
    await: function(run, then) {
      if (typeof then !== "function") throw new Error("agentRun.await requires a continuation");
      var job = parseJson(host.agentRunWait(run));
      return agentRunAwait(job, then);
    },
    cancel: function(run, then) {
      if (typeof then !== "function") throw new Error("agentRun.cancel requires a continuation");
      var job = parseJson(host.agentRunCancel(run));
      return agentRunAwait(job, then);
    },
  };
  var __awaitSeq__ = 0;
  var __awaits__ = {};
  function ioResult(value) {
    return value && typeof value.next === "function" ? host.io.run(value) : value;
  }
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
    __awaits__[k] = {fn:function (value) { return ioResult(then(value)); },job:jobId,owner:awaitInvocationId()};
    return { __ctAwait__: { job: jobId, k: k } };
  }
  host.awaitJob = ctAwaitJob;
  function ioEnvelope(request, then) {
    if (request && request.op === "awaitJob") {
      return host.awaitJob(request.job, function (value) { return then({ value: value }); });
    }
    var started = parseJson(host.ioStart(JSON.stringify(request)));
    if (!started || typeof started.jobId !== "string") return then({ error: (started && started.error) || "host I/O start failed" });
    return host.awaitJob(started.jobId, then);
  }
  function ioError(result) { return result && result.kind === "range" ? new RangeError(result.error) : new Error(result && result.error || "host I/O failed"); }
  host.io = function (request, then) {
    if (typeof then !== "function") throw new Error("host.io requires a continuation");
    return ioEnvelope(request, function (result) {
      if (!result || typeof result.error === "string") throw ioError(result);
      return then(result.value);
    });
  };
  host.io.run = function (steps) {
    function resume(value, failed) {
      var next = failed ? steps.throw(value) : steps.next(value);
      if (next.done) return ioResult(next.value);
      return ioEnvelope(next.value, function (result) {
        return !result || typeof result.error === "string"
          ? resume(ioError(result), true) : resume(result.value, false);
      });
    }
    return resume(undefined, false);
  };
  function* ioRequest(request) { return yield request; }
  host.io.fs = {};
  host.io.surface = {
    call: function* (verb, argsJson) {
      var args = typeof argsJson === "string" ? argsJson : JSON.stringify(argsJson === undefined ? [] : argsJson);
      return yield {op:"surfaceCall", verb:String(verb || ""), args:args};
    },
  };
  ["readFile", "fileExists", "isSymlink", "canonicalPath", "lockFile", "removeFile", "makeDirs", "readDir", "listChildDirs"].forEach(function (op) {
    host.io.fs[op] = function (path) { return ioRequest({ op: op, path: path }); };
  });
  ["readFileHead", "readFileTail"].forEach(function (op) {
    host.io.fs[op] = function (path, maxBytes) { return ioRequest({ op: op, path: path, maxBytes: maxBytes }); };
  });
  ["writeFile", "writeFileAtomic"].forEach(function (op) {
    host.io.fs[op] = function (path, contents) { return ioRequest({ op: op, path: path, contents: contents }); };
  });
  host.io.fs.unlockFile = function (id) { return ioRequest({ op: "unlockFile", id: id }); };
  host.io.fs.writeTempFile = function (contents, suffix) { return ioRequest({ op: "writeTempFile", contents: contents, suffix: suffix || null }); };
  host.io.fs.walkDir = function (path, maxDepth) { return ioRequest({ op: "walkDir", path: path, maxDepth: maxDepth === undefined ? 8 : maxDepth }); };
  host.io.fs.sqliteQuery = function (db, sql, params) { return ioRequest({ op: "sqliteQuery", db: db, sql: sql, params: params || null }); };
  host.io.fs.sqliteQueryNullable = function (db, sql, params) { return ioRequest({ op: "sqliteQuery", db: db, sql: sql, params: params || null, nullable: true }); };
  host.io.fs.readJson = function* (path) { return parseJson(yield { op: "readFile", path: path }); };
  host.io.fs.writeJson = function (path, value) { return ioRequest({ op: "writeFile", path: path, contents: JSON.stringify(value, null, 2) }); };
  host.io.exec = function* (options) {
    var started = yield { op: "execStart", options: options };
    if (!started || typeof started.jobId !== "string") throw ioError(started);
    return yield { op: "awaitJob", job: started.jobId };
  };
  host.io.fetch = function* (options) {
    var started = yield { op: "fetchStart", options: options };
    if (!started || typeof started.jobId !== "string") throw ioError(started);
    return yield { op: "awaitJob", job: started.jobId };
  };
  host.exec = {};
  host.fetch = {};
    function startAndAwait(op, options, then) {
      return ioEnvelope({ op: op, options: options }, function (reply) {
        if (!reply || typeof reply.error === "string") return then({ error: (reply && reply.error) || "host I/O failed" });
        var started = reply.value;
        if (!started || typeof started.jobId !== "string") return then(started || { error: "host job unavailable" });
        return host.awaitJob(started.jobId, then);
      });
    }
    host.exec.async = function (options, then) {
      if (typeof then !== "function") throw new Error("host.exec.async requires a continuation");
      return startAndAwait("execStart", options, then);
    };
    host.fetch.async = function (options, then) {
      if (typeof then !== "function") throw new Error("host.fetch.async requires a continuation");
      return startAndAwait("fetchStart", options, then);
    };
})();
"#;
