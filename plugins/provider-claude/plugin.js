// Claude Code provider plugin.
//
// Pure logic (detection, parsing, slash commands) plus host-API-driven
// I/O (bridge rule install via host.writeFile). Filesystem helpers come
// from the embedded `host` global injected by the JS runtime — see
// docs/plans/relay/20-provider-plugins-ts.md.
//
// Activate with CODETERM_JS_PROVIDERS=claude to let this shadow the
// native provider; default is native.

function manifest() {
  try { return JSON.parse(host.manifestJson() || "null"); } catch (e) { return null; }
}

var TITLE_RE = /claude/i;
// v2.1+ TUIs prefix the title with a braille spinner glyph or ✳/✻.
var BRAILLE_LO = 0x2801;
var BRAILLE_HI = 0x28ff;

var OUTPUT_FINGERPRINTS = [
  "Claude Code",
  "Do you want to proceed?",
  "may I proceed",
  "Would you like me to",
  "Yes, and don't ask again",
  "Enter to confirm",
  "Cogitated for",
  "Brewed for",
  "Reasoned for",
];

var NOISE_FRAGMENTS = [
  "bypass permissions on",
  "shift+tab to cycle",
  "Update available! Run:",
  "brew upgrade claude-code",
  "esc to interrupt",
  "? for shortcuts",
  "ctrl+g to edit",
  "Monthly spend limit",
  "hold Space",
];

var TUI_FRAGMENTS = [
  "? for shortcuts",
  "for shortcuts",
  "ctrl+l",
  "esc to interrupt",
];

var SESSION_EXPLICIT_RE = /session(?:\s+id)?\s*[:=]\s*([a-z]+-[a-z]+-\d{2,6})\b/i;
var SESSION_BARE_RE = /(?:^|\n)\s*([a-z]{4,10}-[a-z]{4,10}-\d{4})\s*(?:$|\n)/m;

function isBrailleSpinner(ch) {
  if (!ch) return false;
  var code = ch.charCodeAt(0);
  return (code >= BRAILLE_LO && code <= BRAILLE_HI) || ch === "✳" || ch === "✻";
}

module.exports.default = {
  detectFromTitle: function (title) {
    var t = String(title || "");
    if (TITLE_RE.test(t)) return true;
    var first = t.charAt(0);
    return isBrailleSpinner(first) && t.length > 2;
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
    for (var i = 0; i < NOISE_FRAGMENTS.length; i++) {
      if (trimmed.indexOf(NOISE_FRAGMENTS[i]) !== -1) return true;
    }
    // All-box-drawing line is decorative chrome.
    for (var j = 0; j < trimmed.length; j++) {
      var c = trimmed[j];
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
    if (c.indexOf("❯") !== -1) return true;
    if (c.indexOf("proceed") !== -1) return true;
    if (c.indexOf("Esc to cancel") !== -1) return true;
    if (c.indexOf("Tab to amend") !== -1) return true;
    if (c.indexOf("1.") !== -1 && c.indexOf("2.") !== -1) return true;
    // "for 0..9" — task completion footers
    for (var i = 0; i < 10; i++) {
      if (c.indexOf(" for " + i) !== -1) return true;
    }
    return false;
  },

  extractSessionName: function (screen) {
    var s = String(screen || "");
    var m = SESSION_EXPLICIT_RE.exec(s);
    if (m && m[1]) return m[1];
    m = SESSION_BARE_RE.exec(s);
    if (m && m[1]) return m[1];
    return null;
  },

  detectEvents: function (text) {
    var t = String(text || "");
    var events = [];
    // PermissionRequest: numbered list + "Esc to cancel" or "❯ N." selector
    var hasNumberedOptions = /^\s*\d+\.\s+/m.test(t) && /^\s*\d+\.\s+/gm.exec(t);
    var numLines = (t.match(/^\s*\d+\.\s+/gm) || []).length;
    var hasEscFooter = t.indexOf("Esc to cancel") !== -1 || t.indexOf("Tab to amend") !== -1;
    var hasSelector = t.indexOf("❯") !== -1;
    if (numLines >= 2 && (hasEscFooter || hasSelector)) {
      events.push("permission_request");
    }
    // TaskComplete: "<CapitalizedVerb> for <digits><s|m|h>"
    var lines = t.split("\n");
    var start = Math.max(0, lines.length - 10);
    for (var i = start; i < lines.length; i++) {
      var line = lines[i].trim();
      var pos = line.indexOf(" for ");
      if (pos < 1) continue;
      var word = line.substring(0, pos);
      var after = line.substring(pos + 5);
      if (!/^[A-Z][a-zA-Z]*$/.test(word)) continue;
      if (/^\d+[smh]/.test(after) || /^\d/.test(after) && /[smh]/.test(after)) {
        events.push("task_complete");
        break;
      }
    }
    return events;
  },

  buildPromptResponse: function (optionIndex) {
    // Claude TUI supports vim j/k navigation — simpler and more reliable
    // than arrow keys. Each step is a single keypress.
    // 'j' = 0x6a → base64 "ag==", '\r' = 0x0d → "DQ=="
    var steps = [];
    for (var i = 0; i < optionIndex; i++) steps.push("ag==");
    steps.push("DQ==");
    return steps;
  },

  // Full launch parity with native: env preamble + per-OS stdin-pipe
  // for headless task delivery + task-file inline for visible mode.
  //
  // Stays portable across macOS/Linux/Windows via host.platform()
  // selection of NULL_DEV, shell quoting, and the BOM prefix that
  // PowerShell needs to parse the temp file as UTF-8.
  buildLaunchCommand: function (params) {
    var p = params || {};
    var win = host.platform() === "windows";
    var unset = win
      ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; "
        + "$OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = "
        + "[System.Text.Encoding]::UTF8; "
        + "$PSNativeCommandArgumentPassing = 'Standard';"
      : "unset CLAUDECODE &&";
    var parts = [unset];

    var useStdinPipe = !!(p.headless && p.task);
    if (useStdinPipe) {
      var path = writeTaskTempFile(p.task);
      var prefix = win
        ? "Get-Content '" + path + "' -Raw -Encoding UTF8 |"
        : "cat '" + path + "' |";
      parts.push(prefix);
      parts.push("claude");
      if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
      parts.push("-p");
      parts.push("--output-format");
      parts.push("stream-json");
      parts.push("--verbose");
      parts.push("--include-partial-messages");
      parts.push("--allowedTools");
      parts.push("Bash");
    } else {
      parts.push("claude");
      if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
      if (p.task) {
        var visiblePath = writeTaskTempFile(p.task);
        parts.push(win
          ? "\"$(Get-Content '" + visiblePath + "' -Raw -Encoding UTF8)\""
          : "\"$(cat " + visiblePath + ")\"");
      }
    }

    if (p.agentName) {
      parts.push("--agent");
      parts.push(host.shell.quoteFor(p.agentName, host.platform()));
    }
    // --session-id is silently ignored by claude in -p mode; only emit
    // it for visible launches.
    if (!useStdinPipe && p.sessionId) {
      parts.push("--session-id");
      parts.push(host.shell.quoteFor(p.sessionId, host.platform()));
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(p.args[i]);
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    var win = host.platform() === "windows";
    var unset = win
      ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; "
        + "$OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = "
        + "[System.Text.Encoding]::UTF8; "
        + "$PSNativeCommandArgumentPassing = 'Standard';"
      : "unset CLAUDECODE &&";
    var parts = [unset, "claude"];
    if (skipPermissions) parts.push("--dangerously-skip-permissions");
    parts.push("--resume");
    parts.push(host.shell.quoteFor(sessionId, host.platform()));
    return parts.join(" ");
  },

  // Walk the session JSONL bottom-up: surface the latest `message.model`
  // OR a "Set model to X" line written by /model. Tail-only — for a
  // multi-MB session, reading the whole file each time would burn CPU
  // on every focused-poll tick.
  detectSessionModel: function (cwd, sessionId) {
    var path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    var contents = host.fs.readFile(path);
    if (!contents) return null;
    var lines = contents.split("\n");
    for (var i = lines.length - 1; i >= 0; i--) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (_) { continue; }
      if (!v || !v.message) continue;
      var raw = contentToText(v.message.content);
      var fam = extractSetModelFamily(raw);
      if (fam) return fam;
      var m = v.message.model;
      if (typeof m === "string" && m && m !== "<synthetic>") {
        return familyFromModelId(m);
      }
    }
    return null;
  },

  // JSONL → SessionMessage. Mirrors Rust agent_claude::parse_session_messages:
  // - skip lines without `type: "user"|"assistant"`
  // - content may be a string OR an array of `{type, text}` blocks
  // - `isMeta` mirrored from JSONL `isMeta` field
  parseSessionMessages: function (text) {
    var out = [];
    var lines = String(text || "").split("\n");
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (e) { continue; }
      var msgType = (v && v.type) || "";
      if (msgType !== "user" && msgType !== "assistant") continue;
      var isMeta = !!(v && v.isMeta);
      var msg = v.message;
      if (!msg) continue;
      var content = msg.content;
      var text2 = "";
      if (typeof content === "string") {
        text2 = content;
      } else if (Array.isArray(content)) {
        var chunks = [];
        for (var j = 0; j < content.length; j++) {
          var b = content[j];
          if (b && b.type === "text" && typeof b.text === "string") {
            chunks.push(b.text);
          }
        }
        text2 = chunks.join(" ");
      } else {
        continue;
      }
      if (!text2 || !text2.trim()) continue;
      var ts = null;
      if (typeof v.timestamp === "string") {
        var d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      out.push({ role: msgType, text: text2, ts: ts, isMeta: isMeta });
    }
    return out;
  },

  // Resolve the JSONL file path for (cwd, sessionId). Same convention
  // as `claudeSessionFilePath` below (the module-level helper); exposed
  // as a plugin method so the Rust `AgentProvider::session_jsonl_path`
  // can ask the plugin instead of duplicating the encoding rule.
  sessionJsonlPath: function (cwd, sessionId) {
    if (!sessionId) return null;
    return claudeSessionFilePath(cwd, sessionId);
  },

  // Writes `~/.claude/rules/codeterm.md` (path declared in manifest.rule.path).
  // Pure plugin code — Rust only provides host.expandHome + host.writeFile.
  installBridgeRule: function (payload) {
    var m = manifest();
    var rulePath = (m && m.rule && m.rule.path) || "~/.claude/rules/codeterm.md";
    var expanded = host.expandHome(rulePath);
    if (!expanded) return { ok: false, error: "could not expand " + rulePath };
    var ok = host.writeFile(expanded, payload);
    if (ok) {
      host.log("info", "[claude-plugin] bridge rule installed to " + expanded);
      return { ok: true };
    }
    return { ok: false, error: "writeFile failed for " + expanded };
  },

  uninstallBridgeRule: function () {
    var m = manifest();
    var rulePath = (m && m.rule && m.rule.path) || "~/.claude/rules/codeterm.md";
    var expanded = host.expandHome(rulePath);
    if (!expanded) return { ok: true }; // nothing to do
    if (!host.fileExists(expanded)) return { ok: true };
    var ok = host.removeFile(expanded);
    if (ok) {
      host.log("info", "[claude-plugin] bridge rule removed from " + expanded);
      return { ok: true };
    }
    return { ok: false, error: "removeFile failed for " + expanded };
  },

  // ── ~/.claude/settings.json hooks integration ───────────────────────
  //
  // Claude POSTs back to CodeTerm on Notification / Stop / PreCompact /
  // SessionStart events. We write curl-based hook entries into the
  // user's settings.json, tagged with `_source: "codeterm-hook-v2"`
  // so we can recognise our own entries on re-install or uninstall.
  //
  // All paths come from manifest.integration.settingsPath; the per-OS
  // NULL_DEV comes from host.platform(). Plugin is 100% portable across
  // OSes — no Rust-side helpers required.

  checkIntegration: function () {
    var m = manifest();
    var path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) {
      return {
        settings_exists: false,
        hooks_installed: false,
        notification_hook: false,
        stop_hook: false,
        precompact_hook: false,
        session_start_hook: false,
      };
    }
    var json = host.fs.readJson(path);
    if (!json) {
      return { settings_exists: true, hooks_installed: false };
    }
    function present(key) {
      var arr = json.hooks && json.hooks[key];
      if (!Array.isArray(arr)) return false;
      for (var i = 0; i < arr.length; i++) {
        var entry = arr[i];
        var hooks = entry && entry.hooks;
        if (!Array.isArray(hooks)) continue;
        for (var j = 0; j < hooks.length; j++) {
          if (hooks[j] && hooks[j]._source === CODETERM_HOOK_MARKER) return true;
        }
      }
      return false;
    }
    var n = present("Notification");
    var s = present("Stop");
    var pc = present("PreCompact");
    var ss = present("SessionStart");
    return {
      settings_exists: true,
      hooks_installed: n && s && pc && ss,
      notification_hook: n,
      stop_hook: s,
      precompact_hook: pc,
      session_start_hook: ss,
    };
  },

  installIntegration: function (apiPort) {
    var m = manifest();
    var path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path) return { ok: false, error: "could not expand settingsPath" };

    var json = host.fileExists(path) ? (host.fs.readJson(path) || {}) : {};
    if (!json.hooks || typeof json.hooks !== "object") json.hooks = {};

    var nullDev = host.platform() === "windows" ? "NUL" : "/dev/null";
    var port = apiPort | 0;
    var authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
    var portExpr = "${CODETERM_API_PORT:-" + port + "}";

    var notifyCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/permission "
      + "-H 'Content-Type: application/json' " + authH + " -d '{}' > " + nullDev + " 2>&1 || true";
    var stopCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/stop "
      + "-H 'Content-Type: application/json' " + authH + " -d '{}' > " + nullDev + " 2>&1 || true";
    var precompactCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/mem/flush -H 'Content-Type: application/json' " + authH
      + ' -d \'{"pane":"\'"$CODETERM_PANE_ID"\'","title":"pre-compact"}\' > '
      + nullDev + " 2>&1 || true";
    var sessionStartCmd = "curl -s -X POST http://localhost:" + portExpr
      + "/api/panes/$CODETERM_PANE_ID/hook/session_start "
      + "-H 'Content-Type: application/json' " + authH + " --data-binary @- > "
      + nullDev + " 2>&1 || true";

    function newEntry(matcher, cmd, timeout) {
      return {
        matcher: matcher,
        hooks: [{
          type: "command",
          command: cmd,
          timeout: timeout,
          _source: CODETERM_HOOK_MARKER,
        }],
      };
    }
    var newHooks = {
      Notification:  newEntry("permission_prompt", notifyCmd, 5000),
      Stop:          newEntry("", stopCmd, 5000),
      PreCompact:    newEntry("", precompactCmd, 10000),
      SessionStart:  newEntry("", sessionStartCmd, 5000),
    };

    // Drop any prior codeterm-installed entries (any version) before
    // appending the current ones — keeps the file from accumulating
    // pre-v2 stragglers.
    var keys = ["Notification", "Stop", "PreCompact", "SessionStart"];
    for (var i = 0; i < keys.length; i++) {
      var k = keys[i];
      if (!Array.isArray(json.hooks[k])) json.hooks[k] = [];
      json.hooks[k] = json.hooks[k].filter(function (entry) {
        return !isCodetermHook(entry);
      });
      json.hooks[k].push(newHooks[k]);
    }

    var ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[claude-plugin] hooks installed at " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },

  uninstallIntegration: function () {
    var m = manifest();
    var path = host.expandHome((m && m.integration && m.integration.settingsPath)
      || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) return { ok: true };
    var json = host.fs.readJson(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") return { ok: true };
    // Native impl only drops Notification + Stop on uninstall (keeps
    // PreCompact + SessionStart alone — historical behaviour for mem
    // continuity). Mirror that.
    var keys = ["Notification", "Stop"];
    for (var i = 0; i < keys.length; i++) {
      var k = keys[i];
      if (!Array.isArray(json.hooks[k])) continue;
      json.hooks[k] = json.hooks[k].filter(function (entry) {
        return !isCodetermHook(entry);
      });
    }
    var ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[claude-plugin] hooks removed from " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },
};

// ── Session-file / model-label helpers ────────────────────────────────
//
// Native parity: `~/.claude/projects/<dash-encoded-cwd>/<sid>.jsonl`.
// dash_encoded means '/', '\', ':' all become '-'. Matches the native
// `crate::plugin::js_runtime::manifest::encode_cwd_dashed` path that
// JsProvider uses for session_file_path.
function claudeSessionFilePath(cwd, sessionId) {
  var m = manifest();
  var root = (m && m.sessions && m.sessions.rootDir) || "~/.claude/projects";
  var ext = (m && m.sessions && m.sessions.fileExtension) || "jsonl";
  var expanded = host.expandHome(root);
  if (!expanded) return null;
  var encoded = encodeCwdDashed(cwd);
  var sep = host.platform() === "windows" ? "\\" : "/";
  return expanded + sep + encoded + sep + sessionId + "." + ext;
}

function encodeCwdDashed(cwd) {
  // Native parity: every non-alphanumeric char → '-'. Matches
  // JsProvider::encode_cwd_dashed exactly.
  return String(cwd || "").replace(/[^a-zA-Z0-9]/g, "-");
}

function contentToText(c) {
  if (typeof c === "string") return c;
  if (!Array.isArray(c)) return "";
  var chunks = [];
  for (var i = 0; i < c.length; i++) {
    var b = c[i];
    if (b && typeof b.text === "string") chunks.push(b.text);
  }
  return chunks.join(" ");
}

function extractSetModelFamily(raw) {
  var idx = raw.indexOf("Set model to ");
  if (idx === -1) return null;
  var after = raw.substring(idx + "Set model to ".length);
  var cleaned = stripAnsi(after).replace(/^\s+/, "");
  var fam = "";
  for (var i = 0; i < cleaned.length; i++) {
    var c = cleaned.charAt(i);
    if (!/[a-zA-Z]/.test(c)) break;
    fam += c;
  }
  return fam ? capitalize(fam) : null;
}

function stripAnsi(s) {
  var out = "";
  var i = 0;
  while (i < s.length) {
    if (s.charCodeAt(i) === 0x1b && s.charAt(i + 1) === "[") {
      i += 2;
      while (i < s.length && !/[a-zA-Z]/.test(s.charAt(i))) i++;
      if (i < s.length) i++;
    } else {
      out += s.charAt(i);
      i++;
    }
  }
  return out;
}

function familyFromModelId(id) {
  var m = String(id || "").toLowerCase();
  var fams = ["opus", "sonnet", "haiku"];
  for (var i = 0; i < fams.length; i++) {
    if (m.indexOf(fams[i]) !== -1) return capitalize(fams[i]);
  }
  var body = m.indexOf("claude-") === 0 ? m.substring(7) : m;
  var head = body.split("-")[0] || body;
  return capitalize(head);
}

function capitalize(s) {
  if (!s) return "";
  return s.charAt(0).toUpperCase() + s.substring(1).toLowerCase();
}

// ── Task-file helpers ─────────────────────────────────────────────────
//
// Native ShellSyntax::write_task_temp uses a short uuid suffix and on
// Windows prefixes the file with a UTF-8 BOM so PowerShell parses the
// content as UTF-8 regardless of system locale. host.fs.writeTempFile
// gives us the path; we control the bytes inside.
function writeTaskTempFile(task) {
  var suffix = ".ct-task-" + randomShortHex(8);
  var win = host.platform() === "windows";
  // Windows PowerShell parses files as UTF-8 only when they start
  // with the U+FEFF byte-order mark — prepend it so non-ASCII task
  // bodies survive the round-trip through `Get-Content -Encoding UTF8`.
  var body = win ? "﻿" + task : task;
  return host.fs.writeTempFile(body, suffix);
}

function randomShortHex(len) {
  var hex = "0123456789abcdef";
  var out = "";
  for (var i = 0; i < len; i++) out += hex.charAt((Math.random() * 16) | 0);
  return out;
}

// ── Hook recognition helpers ──────────────────────────────────────────
var CODETERM_HOOK_MARKER = "codeterm-hook-v2";

function isCodetermHook(entry) {
  var hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (var i = 0; i < hooks.length; i++) {
    var h = hooks[i];
    if (!h) continue;
    if (typeof h._source === "string"
        && (h._source.indexOf("codeterm-hook") === 0 || h._source === "codeterm")) {
      return true;
    }
    var cmd = h.command;
    if (typeof cmd === "string") {
      var isUrl = cmd.indexOf("7685") !== -1 || cmd.indexOf("7686") !== -1;
      var isPath = cmd.indexOf("/api/notifications") !== -1
        || cmd.indexOf("/api/panes/") !== -1
        || cmd.indexOf("/hook/permission") !== -1
        || cmd.indexOf("/hook/stop") !== -1
        || cmd.indexOf("/hook/session_start") !== -1
        || cmd.indexOf("/api/mem/flush") !== -1;
      if (isUrl && isPath) return true;
    }
  }
  return false;
}
