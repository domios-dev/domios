// Codex provider plugin: detect/parse/slash commands plus rollout-tree session discovery.

var CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
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
var OUTPUT_THRESHOLD = 2;
var TUI_FRAGMENTS = ["openai codex", "ctrl+c to cancel"];

function outputScore(text) {
  var score = 0;
  if (/^\s*(?:›|>|▶)\s+\S+/m.test(text)) score += 2;
  if (/^\s*(?:\[[^\]]+\]\s*)?codex[/>]/im.test(text)) score += 1;
  if (/ctrl\+c to cancel/i.test(text)) score += 1;
  for (var i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
    if (text.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) score += 1;
  }
  return score;
}

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
      parts.push("--dangerously-bypass-hook-trust");
      if (p.skipPermissions) {
        parts.push("--dangerously-bypass-approvals-and-sandbox");
      }
      parts.push("-c");
      parts.push("'shell_environment_policy.inherit=all'");
      parts.push("--skip-git-repo-check");
      parts.push(host.shell.quote(p.task));
    } else {
      parts.push("codex");
      parts.push("--dangerously-bypass-hook-trust");
      if (p.skipPermissions) {
        parts.push("--dangerously-bypass-approvals-and-sandbox");
      }
    }
    if (p.args && p.args.length > 0) {
      // Quote each arg — caller-supplied via the spawn API; prevents shell injection.
      for (var i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    var parts = ["codex", "resume"];
    parts.push("--dangerously-bypass-hook-trust");
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    // session_id accepts UUID or rollout filename stem.
    var uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quote(uuid));
    return parts.join(" ");
  },

  // Rollout-tree detection: scan `~/.codex/sessions/.../rollout-*.jsonl`
  // and read only `session_meta` headers on first file match.

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

  // Chat uses generic JSONL-session hook; codex paths resolve via sessionFilePath.
  sessionJsonlPath: function (cwd, sessionId) {
    return this.sessionFilePath(cwd, sessionId);
  },

    // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts. Change both or the test fails.
  // Parse codex rollout JSONL into RichMessages. Codex schema:
  //   {type:"response_item", payload:{type:"message"|"function_call"|"function_call_output", ...}}
  // We read response_item lines only (event_msg duplicates them; reasoning is
  // encrypted; token_count/turn_context/session_meta are metadata).
  parseSessionDelta: function (chunk) {
    var out = [];
    var lines = String(chunk || "").split("\n");
    var hadInvalid = false;
    for (var i = 0; i < lines.length; i++) {
      if (!lines[i]) continue;
      var v;
      try { v = JSON.parse(lines[i]); } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!validateEntry(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!v || v.type !== "response_item" || !v.payload) continue;
      var p = v.payload;
      var ts = null;
      if (typeof v.timestamp === "string") {
        var d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      if (p.type === "message") {
        var role = p.role === "assistant" ? "assistant" : (p.role === "user" ? "user" : null);
        if (!role) continue; // skip developer/system noise
        var text = "";
        if (Array.isArray(p.content)) {
          for (var j = 0; j < p.content.length; j++) {
            var c = p.content[j];
            if (c && typeof c.text === "string") text += c.text;
          }
        } else if (typeof p.content === "string") {
          text = p.content;
        }
        if (!text) continue;
        out.push({ role: role, blocks: [{ kind: "text", data: { text: text } }], ts: ts });
      } else if (p.type === "function_call") {
        var input = {};
        try { input = JSON.parse(p.arguments || "{}"); } catch (_) { input = { raw: p.arguments }; }
        if (input && input.cmd && !input.command) input.command = input.cmd; // surface for generic readers
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: p.name || "tool", input: input } }],
          ts: ts,
        });
      } else if (p.type === "function_call_output") {
        var content = p.output;
        if (content && typeof content === "object") content = JSON.stringify(content);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: String(content == null ? "" : content) } }],
          ts: ts,
        });
      }
    }
    return { messages: out };
  },

  detectSessionId: function (cwd, exclude, maxAgeMs) {
    return null;
  },

  checkIntegration: function () {
    var path = codexHooksPath();
    if (!path || !host.fs.fileExists(path)) {
      return { hooks_file_exists: false, hooks_installed: false, session_start_hook: false };
    }
    var json = host.fs.readJson(path);
    if (!json) {
      return { hooks_file_exists: true, hooks_installed: false, session_start_hook: false };
    }
    var arr = json.hooks && json.hooks.SessionStart;
    var installed = Array.isArray(arr) && arr.some(isCodetermCodexHook);
    var windowsReady = host.platform() !== "windows" || (
      !!codexWindowsHookScriptPath() &&
      host.fs.fileExists(codexWindowsHookScriptPath()) &&
      Array.isArray(arr) &&
      arr.some(hasCodetermCodexWindowsCommand)
    );
    return {
      hooks_file_exists: true,
      hooks_installed: installed && windowsReady,
      session_start_hook: installed && windowsReady,
    };
  },

  installIntegration: function (apiPort) {
    var path = codexHooksPath();
    if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };

    var json = host.fs.fileExists(path) ? (host.fs.readJson(path) || {}) : {};
    if (!json.hooks || typeof json.hooks !== "object") json.hooks = {};
    if (!Array.isArray(json.hooks.SessionStart)) json.hooks.SessionStart = [];
    json.hooks.SessionStart = json.hooks.SessionStart.filter(function (entry) {
      return !isCodetermCodexHook(entry);
    });

    var nullDev = host.platform() === "windows" ? "NUL" : "/dev/null";
    var port = apiPort | 0;
    var portExpr = "${CODETERM_API_PORT:-" + port + "}";
    var authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
    var cmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/session_start "
      + "-H 'Content-Type: application/json' "
      + "-H 'X-CodeTerm-Agent-Provider: codex' "
      + authH
      + " --data-binary @- > " + nullDev + " 2>&1 || true";
    var scriptPath = codexWindowsHookScriptPath();
    var commandWindows = scriptPath
      ? 'powershell -NoProfile -ExecutionPolicy Bypass -File "' + scriptPath + '"'
      : cmd;

    if (scriptPath && !host.fs.writeFile(scriptPath, codexWindowsHookScript(port))) {
      return { ok: false, error: "writeFile failed for " + scriptPath };
    }

    json.hooks.SessionStart.push({
      matcher: "startup|resume|clear|compact",
      hooks: [{
        type: "command",
        command: cmd,
        commandWindows: commandWindows,
        timeout: 5,
        _source: CODETERM_CODEX_HOOK_MARKER,
      }],
    });

    var ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[codex-plugin] SessionStart hook installed at " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },

  uninstallIntegration: function () {
    var path = codexHooksPath();
    if (!path || !host.fs.fileExists(path)) return { ok: true };
    var json = host.fs.readJson(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") return { ok: true };
    if (Array.isArray(json.hooks.SessionStart)) {
      json.hooks.SessionStart = json.hooks.SessionStart.filter(function (entry) {
        return !isCodetermCodexHook(entry);
      });
    }
    var ok = host.fs.writeJson(path, json);
    var scriptPath = codexWindowsHookScriptPath();
    if (scriptPath && host.fs.fileExists(scriptPath)) host.fs.removeFile(scriptPath);
    return ok ? { ok: true } : { ok: false, error: "writeJson failed for " + path };
  },

  ensureIntegrationOnStartup: function (apiPort) {
    var status = this.checkIntegration();
    if (status && status.hooks_installed) return { ok: true };
    return this.installIntegration(apiPort);
  },

  // Parse the `/status` TUI screen into a UsageSnapshot-shaped object
  // (snake_case fields). Gauges report "% left"; snapshot stores "% used".
  parseUsage: function (rawText, nowMs) {
    var raw = String(rawText || "");
    if (!raw.trim()) return null;
    var text = raw.split("\r").join("").split("█").join(" ").split("░").join(" ");
    var session = codexParseGauge(text, "5h\\s*limit", false, nowMs);
    var weekly = codexParseGauge(text, "Weekly\\s*limit", true, nowMs);
    if (session.pct === null && weekly.pct === null) return null;
    return {
      provider: "codex",
      account_id: null,
      captured_at_ms: nowMs,
      session_pct: session.pct,
      session_resets_at_ms: session.resetsMs,
      weekly_pct: weekly.pct,
      weekly_resets_at_ms: weekly.resetsMs,
      raw_text: raw,
    };
  },

  detectActiveSession: function (cwd, currentSid) {
    return null;
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

  // Read tail-most `turn_context.payload.model` and map to short model labels.
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

  // Mirror native parse_session_messages for response_item message blocks.
  parseSessionMessages: function (text) {
    var out = [];
    var lines = String(text || "").split("\n");
    var hadInvalid = false;
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (e) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!isValidCodexSessionMessage(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
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

// Use function expressions for QuickJS strict-mode compatibility with bundler rewrites.
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

var codexHooksPath = function () {
  var home = host.homeDir();
  if (!home) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "hooks.json";
};

var codexWindowsHookScriptPath = function () {
  var home = host.homeDir();
  if (!home) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "codeterm-session-start.ps1";
};

var codexWindowsHookScript = function (apiPort) {
  return [
    "# " + CODETERM_CODEX_HOOK_MARKER,
    '$ErrorActionPreference = "SilentlyContinue"',
    "$pane = $env:CODETERM_PANE_ID",
    "if ([string]::IsNullOrWhiteSpace($pane)) { exit 0 }",
    '$port = if ($env:CODETERM_API_PORT) { $env:CODETERM_API_PORT } else { "' + String(apiPort || 7686) + '" }',
    '$base = if ($env:CODETERM_API_URL) { $env:CODETERM_API_URL.TrimEnd("/") } else { "http://127.0.0.1:$port" }',
    "$headers = @{",
    '  "Content-Type" = "application/json"',
    '  "X-CodeTerm-Agent-Provider" = "codex"',
    "}",
    "if ($env:CODETERM_API_TOKEN) {",
    '  $headers["Authorization"] = "Bearer $($env:CODETERM_API_TOKEN)"',
    "}",
    "$body = [Console]::In.ReadToEnd()",
    'Invoke-RestMethod -Method Post -Uri "$base/api/panes/$pane/hook/session_start" -Headers $headers -Body $body -TimeoutSec 5 | Out-Null',
    "exit 0",
    "",
  ].join("\n");
};

var isCodetermCodexHook = function (entry) {
  var hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (var i = 0; i < hooks.length; i++) {
    var hook = hooks[i];
    if (!hook) continue;
    if (hook._source === CODETERM_CODEX_HOOK_MARKER) return true;
    var cmd = String(hook.command || "");
    if (
      cmd.indexOf("/hook/session_start") !== -1 &&
      cmd.indexOf("X-CodeTerm-Agent-Provider: codex") !== -1
    ) {
      return true;
    }
  }
  return false;
};

var hasCodetermCodexWindowsCommand = function (entry) {
  var hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (var i = 0; i < hooks.length; i++) {
    var hook = hooks[i];
    if (!hook || hook._source !== CODETERM_CODEX_HOOK_MARKER) continue;
    var command = String(hook.commandWindows || "");
    if (command.indexOf("codeterm-session-start.ps1") !== -1) {
      return true;
    }
  }
  return false;
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

// Shorten common model ids for tab badges (Codex / GPT / Sonnet etc.).
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

// Read first JSONL line, return {sessionId, cwd} only when type is `session_meta`.
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

// ── /status screen parsing (port of native status_tui) ──────────────

function codexParseGauge(text, marker, isWeekly, nowMs) {
  var re = new RegExp(
    marker + "[\\s\\S]{0,200}?(\\d+(?:\\.\\d+)?)\\s*%\\s*left[\\s\\S]*?\\(resets?\\s+([^)]+?)\\)",
  );
  var caps = re.exec(text);
  if (!caps) return { pct: null, resetsMs: null };
  var left = parseFloat(caps[1]);
  var pct = isNaN(left) ? null : Math.min(100, Math.max(0, 100 - left));
  var resetsRaw = (caps[2] || "").trim();
  var resetsMs = isWeekly
    ? codexParseWeeklyResets(resetsRaw, nowMs)
    : codexParseSessionResets(resetsRaw, nowMs);
  return { pct: pct, resetsMs: resetsMs };
}

function codexParseSessionResets(raw, nowMs) {
  var m = /(\d{1,2}):(\d{2})/.exec(String(raw || "").trim());
  if (!m) return null;
  var hh = parseInt(m[1], 10), mm = parseInt(m[2], 10);
  var now = new Date(nowMs);
  var cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hh, mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1000);
  return cand.getTime();
}

var CODEX_MONTHS = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7,
  sep: 8, sept: 8, september: 8, oct: 9, october: 9, nov: 10, november: 10,
  dec: 11, december: 11,
};

function isValidCodexSessionDelta(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  var kind = v.payload.type;
  if (kind !== "message" && kind !== "function_call" && kind !== "function_call_output") return false;
  return true;
}

function validateEntry(v) {
  return isValidCodexSessionDelta(v);
}

function isValidCodexSessionMessage(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  if (v.payload.type !== "message") return false;
  if (v.payload.role !== "user" && v.payload.role !== "assistant") return false;
  if (!Array.isArray(v.payload.content)) return false;
  return true;
}

function codexParseWeeklyResets(raw, nowMs) {
  var m = /(\d{1,2}):(\d{2})\s+on\s+(\d{1,2})\s+([A-Za-z]+)/.exec(String(raw || "").trim());
  if (!m) return null;
  var hh = parseInt(m[1], 10), mm = parseInt(m[2], 10);
  var day = parseInt(m[3], 10);
  var month = CODEX_MONTHS[m[4].toLowerCase()];
  if (month === undefined) return null;
  var now = new Date(nowMs);
  var cand = new Date(now.getFullYear(), month, day, hh, mm, 0, 0);
  if (cand.getTime() <= nowMs) {
    cand = new Date(now.getFullYear() + 1, month, day, hh, mm, 0, 0);
  }
  return cand.getTime();
}
