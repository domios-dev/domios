// Codex provider plugin. Detection + slash commands + rollout-tree
// session enumeration via host.fs.walkDir + host.fs.sqliteQuery for
// the modern sqlite-backed install.

var TITLE_RE = /codex/i;
var OUTPUT_FINGERPRINTS = [
  "OpenAI Codex",
  "permissions: YOLO mode",
  "permissions: read-only",
  "Would you like to run the following command",
  "Would you like to make the following edits",
  "approve network access",
  "Would you like to grant these permissions",
  "codex>",
];
var TUI_FRAGMENTS = ["openai codex", "ctrl+c to cancel"];

module.exports.default = {
  detectFromTitle: function (title) {
    return TITLE_RE.test(String(title || ""));
  },

  detectFromOutput: function (text) {
    var t = String(text || "");
    for (var i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },

  screenHasTui: function (screen) {
    var lower = String(screen || "").toLowerCase();
    for (var i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
  },

  isOutputNoise: function (text) {
    var trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (var i = 0; i < trimmed.length; i++) {
      var c = trimmed[i];
      if (
        c !== "─" && c !== "━" && c !== "═" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  hasEventMarkers: function (chunk) {
    var c = String(chunk || "");
    return (
      c.indexOf("Would you like") !== -1 ||
      c.indexOf("approve") !== -1 ||
      c.indexOf("run the following") !== -1 ||
      c.indexOf("make the following") !== -1 ||
      c.indexOf("grant these permissions") !== -1
    );
  },

  detectEvents: function (text) {
    var t = String(text || "");
    var events = [];
    if (
      t.indexOf("Would you like to run the following command") !== -1 ||
      t.indexOf("Would you like to make the following edits") !== -1 ||
      t.indexOf("Do you want to approve network access") !== -1 ||
      t.indexOf("Would you like to grant these permissions") !== -1
    ) {
      events.push("permission_request");
    }
    return events;
  },

  buildLaunchCommand: function (params) {
    var p = params || {};
    var parts = [];
    if (p.headless && p.task) {
      parts.push("codex");
      parts.push("exec");
      if (p.skipPermissions) {
        parts.push("--dangerously-bypass-approvals-and-sandbox");
      }
      parts.push("-c");
      parts.push("'shell_environment_policy.inherit=all'");
      parts.push("--skip-git-repo-check");
      parts.push(host.shell.quote(p.task));
    } else {
      parts.push("codex");
      if (p.skipPermissions) {
        parts.push("--dangerously-bypass-approvals-and-sandbox");
      }
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(p.args[i]);
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    var parts = ["codex", "resume"];
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    // session_id may be a UUID OR a rollout filename stem; native impl
    // extracts the UUID. We do the same so callers can pass either.
    var uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quote(uuid));
    return parts.join(" ");
  },

  // ── Rollout-tree session detection ─────────────────────────────────
  //
  // Codex stores sessions as `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`.
  // Each file's first line is `session_meta` with the authoritative
  // session UUID + cwd. We walk the tree once per call and lazily
  // peek each candidate's header — cheap enough for the volumes
  // observed in practice (hundreds of sessions, not thousands).

  sessionFilePath: function (_cwd, sessionId) {
    var root = codexSessionsRoot();
    if (!root) return null;
    var hit = null;
    var files = host.fs.walkDir(root, 5);
    for (var i = 0; i < files.length; i++) {
      var f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      var stem = f.name.substring(0, f.name.length - 6);
      if (stem === sessionId || extractSessionUuid(stem) === sessionId) {
        hit = f.path;
        break;
      }
    }
    return hit;
  },

  detectSessionId: function (cwd, exclude, maxAgeMs) {
    var root = codexSessionsRoot();
    if (!root || !host.fs.fileExists(root)) return null;
    var nowMs = host.unixNowMs();
    var cutoff = maxAgeMs > 0 ? nowMs - maxAgeMs : null;
    var excluded = exclude || [];
    var candidates = [];
    var files = host.fs.walkDir(root, 5);
    for (var i = 0; i < files.length; i++) {
      var f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      if (cutoff !== null && f.modifiedMs !== null && f.modifiedMs < cutoff) continue;
      var meta = readSessionMeta(f.path);
      if (!meta) continue;
      if (meta.cwd && meta.cwd !== cwd) continue;
      var stem = f.name.substring(0, f.name.length - 6);
      var sid = meta.sessionId || extractSessionUuid(stem);
      if (!sid) continue;
      if (excluded.indexOf(sid) !== -1) continue;
      candidates.push({ sid: sid, ts: f.modifiedMs || f.createdMs || 0 });
    }
    if (candidates.length === 0) return null;
    candidates.sort(function (a, b) { return b.ts - a.ts; });
    return candidates[0].sid;
  },

  enumerateSessions: function () {
    var root = codexSessionsRoot();
    if (!root || !host.fs.fileExists(root)) return [];
    var out = [];
    var files = host.fs.walkDir(root, 5);
    for (var i = 0; i < files.length; i++) {
      var f = files[i];
      if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
      var stem = f.name.substring(0, f.name.length - 6);
      var meta = readSessionMeta(f.path);
      var sid = (meta && meta.sessionId) || extractSessionUuid(stem);
      if (!sid) continue;
      out.push({
        sessionId: sid,
        provider: "codex",
        projectPath: meta && meta.cwd ? meta.cwd : null,
        title: null,
        summary: null,
        filePath: f.path,
        createdTs: f.createdMs ? Math.floor(f.createdMs / 1000) : null,
        modifiedTs: f.modifiedMs ? Math.floor(f.modifiedMs / 1000) : null,
      });
    }
    return out;
  },

  // Latest `turn_context.payload.model` in the JSONL tail. Codex writes
  // a turn_context record at the start of every assistant turn — so the
  // last one wins. Short-label result ("Codex" / "GPT-5.5" / "Sonnet")
  // matches the native badge convention.
  detectSessionModel: function (cwd, sessionId) {
    var path = this.sessionFilePath(cwd, sessionId);
    if (!path) return null;
    var contents = host.fs.readFile(path);
    if (!contents) return null;
    var lines = contents.split("\n");
    for (var i = lines.length - 1; i >= 0; i--) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (_) { continue; }
      if (!v || v.type !== "turn_context") continue;
      var m = v.payload && v.payload.model;
      if (typeof m === "string" && m) return codexShortLabel(m);
    }
    return null;
  },

  // Mirrors native parse_session_messages: response_item type with
  // payload.type === "message", role user|assistant, content blocks of
  // type input_text|output_text|text.
  parseSessionMessages: function (text) {
    var out = [];
    var lines = String(text || "").split("\n");
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (e) { continue; }
      if (!v || v.type !== "response_item") continue;
      var payload = v.payload;
      if (!payload || payload.type !== "message") continue;
      var role = payload.role;
      if (role !== "user" && role !== "assistant") continue;
      var content = payload.content;
      if (!Array.isArray(content)) continue;
      var chunks = [];
      for (var j = 0; j < content.length; j++) {
        var b = content[j];
        if (!b) continue;
        if (
          (b.type === "input_text" ||
            b.type === "output_text" ||
            b.type === "text") &&
          typeof b.text === "string"
        ) {
          chunks.push(b.text);
        }
      }
      var text2 = chunks.join(" ");
      if (!text2.trim()) continue;
      var ts = null;
      if (typeof v.timestamp === "string") {
        var d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      out.push({ role: role, text: text2, ts: ts, isMeta: false });
    }
    return out;
  },
};

// Function expressions (not declarations) — keep QuickJS strict-mode
// parser happy and avoid the "function declarations can't appear in
// single-statement context" error when bundlers re-wrap our source.
var endsWith = function (s, suffix) {
  return s.length >= suffix.length
    && s.substring(s.length - suffix.length) === suffix;
};

var codexSessionsRoot = function () {
  var override = host.envGet("CODETERM_CODEX_SESSIONS_ROOT");
  if (override) return override;
  var home = host.homeDir();
  if (!home) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "sessions";
};

// rollout-YYYY-MM-DDTHH-MM-SS-<uuid> → uuid
var extractSessionUuid = function (stem) {
  if (typeof stem !== "string") return null;
  var prefix = "rollout-";
  if (stem.indexOf(prefix) !== 0) return null;
  var rest = stem.substring(prefix.length);
  if (rest.length <= 20) return null;
  if (rest.charAt(19) !== "-") return null;
  return rest.substring(20);
};

// Short tab-badge label for a codex model id ("gpt-5-codex" → "Codex",
// "gpt-5" → "GPT-5", "claude-sonnet-4-6" → "Sonnet").
var codexShortLabel = function (modelId) {
  var m = String(modelId || "").toLowerCase();
  if (m.indexOf("codex") !== -1) return "Codex";
  var fams = ["opus", "sonnet", "haiku", "gemini"];
  for (var i = 0; i < fams.length; i++) {
    if (m.indexOf(fams[i]) !== -1) {
      return fams[i].charAt(0).toUpperCase() + fams[i].substring(1);
    }
  }
  if (m.indexOf("gpt") === 0) {
    var head = m.split("-").slice(0, 2).join("-");
    return "GPT" + head.substring(3);
  }
  var first = m.split("-")[0] || m;
  return first.charAt(0).toUpperCase() + first.substring(1);
};

// Read first line, parse as JSON, return {sessionId, cwd} when the
// header is a session_meta record. Returns null otherwise.
var readSessionMeta = function (path) {
  var contents = host.fs.readFile(path);
  if (!contents) return null;
  var firstNl = contents.indexOf("\n");
  var line = firstNl === -1 ? contents : contents.substring(0, firstNl);
  var v;
  try { v = JSON.parse(line.trim()); } catch (_) { return null; }
  if (!v || v.type !== "session_meta") return null;
  var p = v.payload || {};
  return {
    sessionId: typeof p.id === "string" ? p.id : null,
    cwd: typeof p.cwd === "string" ? p.cwd : null,
  };
};
