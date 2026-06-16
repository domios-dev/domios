// Claude Code provider plugin: detection/parsing/slash commands in plugin logic,
// host APIs for side effects like bridge rule install (`host.writeFile`).

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
var OUTPUT_THRESHOLD = 2;

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
  // v2.1+ idle footer
  "bypass permissions",
  "shift+tab to cycle",
];

var SESSION_EXPLICIT_RE = /session(?:\s+id)?\s*[:=]\s*([a-z]+-[a-z]+-\d{2,6})\b/i;
var SESSION_BARE_RE = /(?:^|\n)\s*([a-z]{4,10}-[a-z]{4,10}-\d{4})\s*(?:$|\n)/m;
var CLAUDE_SESSION_DELTA_TYPES = { system: true, user: true, assistant: true };


function hasSpinnerGlyph(text) {
  return /(?:^|\n)\s*[⠀-⣿✱✳✻]\s+\S/.test(text);
}

function hasFooterLayout(text) {
  return /(?:^|\n).*(?:\? for shortcuts|for shortcuts).*(?:esc to interrupt|shift\+tab|ctrl\+g)/i.test(text);
}

function hasPromptOptionLayout(text) {
  var lines = String(text || "").split("\n");
  var numbered = 0;
  for (var i = 0; i < lines.length; i++) {
    if (/^\s*(?:❯\s*)?\d+\.\s+\S+/.test(lines[i])) numbered++;
  }
  return numbered >= 2 && (text.indexOf("❯") !== -1 || /Esc .*?(cancel|back)|Enter .*?confirm|Tab .*?amend/i.test(text));
}

function outputScore(text) {
  var score = 0;
  if (hasSpinnerGlyph(text)) score += 2;
  if (hasFooterLayout(text)) score += 1;
  if (hasPromptOptionLayout(text)) score += 2;
  for (var i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
    if (text.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) score += 1;
  }
  return score;
}
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
    var s = String(screen || "");
    var lower = s.toLowerCase();
    for (var i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    // Banner logo quadrant blocks — never in plain shell output.
    return s.indexOf("▛") !== -1 || s.indexOf("▜") !== -1;
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
  // Claude TUI uses vim-style single-key navigation.
    var steps = [];
    for (var i = 0; i < optionIndex; i++) steps.push("ag==");
    steps.push("DQ==");
    return steps;
  },

  // Launch command parity for native behavior across OS-specific execution modes.
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
    }

    if (p.agentName) {
      parts.push("--agent");
      parts.push(host.shell.quoteFor(p.agentName, host.platform()));
    }
    // --session-id is ignored in -p mode; emit only for visible launches.
    if (!useStdinPipe && p.sessionId) {
      parts.push("--session-id");
      parts.push(host.shell.quoteFor(p.sessionId, host.platform()));
    }
    if (p.args && p.args.length > 0) {
      // Quote each arg — they reach a shell, and `args` is caller-supplied via
      // the spawn API (a mesh peer with SpawnAgents could inject `; cmd`).
      for (var i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    // Visible mode passes the prompt as a positional, last, after `--`: a spawn
    // task can start with the -=-codeterm:...-=- origin marker, which commander
    // would otherwise reject as an unknown option.
    if (!useStdinPipe && p.task) {
      var visiblePath = writeTaskTempFile(p.task);
      parts.push("--");
      parts.push(win
        ? "\"$(Get-Content '" + visiblePath + "' -Raw -Encoding UTF8)\""
        : "\"$(cat " + visiblePath + ")\"");
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

  // Tail session JSONL to find latest model (`message.model` or `Set model to ...`).
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

  // Parse the `/usage` TUI screen into a UsageSnapshot-shaped object
  // (snake_case fields, matching the Rust struct). Returns null when nothing
  // recognizable was found.
  parseUsage: function (rawText, nowMs) {
    var raw = String(rawText || "");
    if (!raw.trim()) return null;
    var text = usageNormalize(raw);

    var session = usageParseBlock(text, "Current session", false, nowMs);
    var weekly = usageParseBlock(text, "Current week\\s*\\(\\s*all\\s*models\\s*\\)", true, nowMs);

    if (session.pct === null && weekly.pct === null) return null;
    return {
      provider: "claude",
      account_id: null,
      captured_at_ms: nowMs,
      session_pct: session.pct,
      session_resets_at_ms: session.resetsMs,
      weekly_pct: weekly.pct,
      weekly_resets_at_ms: weekly.resetsMs,
      raw_text: raw,
    };
  },

  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts. Change both or the test fails.
  // Parse `user`/`assistant` lines and map content blocks to SessionMessage.
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
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!isValidClaudeSessionMessage(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
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

  // Rust provides complete lines; plugin parses JSON and builds RichMessage[].
  // Byte offsets stay in Rust.
  parseSessionDelta: function (chunk) {
    var out = [];
    var lines = String(chunk || "").split("\n");
    var hadInvalid = false;
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      if (!line) continue;
      var v;
      try { v = JSON.parse(line); } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!validateEntry(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      var t = v && v.type;
      var blocks = [];
      var role;
      if (t === "system") {
        // System rows carry top-level `content` (string/array) + `subtype`.
        // The host normalizer turns them into system / local_command cards.
        role = "system";
        var sysText = (typeof v.content === "string")
          ? v.content
          : (Array.isArray(v.content)
              ? v.content.filter(function (c) { return c && c.type === "text"; })
                  .map(function (c) { return c.text || ""; }).join("\n")
              : "");
        var sdata = { text: sysText };
        if (typeof v.subtype === "string") sdata.subtype = v.subtype;
        blocks.push({ kind: "text", data: sdata });
      } else if (t === "user" || t === "assistant") {
        role = t;
        var msg = v.message;
        if (!msg) continue;
        var content = msg.content;
        if (typeof content === "string") {
          if (content) blocks.push({ kind: "text", data: { text: content } });
        } else if (Array.isArray(content)) {
          for (var j = 0; j < content.length; j++) {
            var b = content[j];
            if (!b || typeof b.type !== "string") continue;
            var data = {};
            for (var k in b) {
              if (Object.prototype.hasOwnProperty.call(b, k) && k !== "type") {
                data[k] = b[k];
              }
            }
            blocks.push({ kind: b.type, data: data });
          }
        } else {
          continue;
        }
      } else {
        continue;
      }
      if (!blocks.length) continue;
      var ts = null;
      if (typeof v.timestamp === "string") {
        var d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1000);
      }
      out.push({
        role: role,
        blocks: blocks,
        ts: ts,
        uuid: (typeof v.uuid === "string") ? v.uuid : null,
        isMeta: v.isMeta === true ? true : null,
        timestamp: (typeof v.timestamp === "string") ? v.timestamp : null,
      });
    }
    return { messages: out };
  },

  // Keep JSONL path encoding aligned with `claudeSessionFilePath`.
  sessionJsonlPath: function (cwd, sessionId) {
    if (!sessionId) return null;
    return claudeSessionFilePath(cwd, sessionId);
  },

  // Write bridge rule to `~/.claude/rules/codeterm.md` via host APIs.
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

  // Manage Claude settings hooks for Notification, Stop, PreCompact, SessionStart.
  // Hooks use `_source: "codeterm-hook-v2"` and manifest paths for stable lifecycle.

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

    // Remove existing codeterm hooks before appending to avoid stale leftovers.
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
    // Uninstall removes Notification + Stop only; keeps PreCompact + SessionStart.
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

// Session-file/model-label helpers aligned with native `~/.claude/projects`.
// `encode_cwd_dashed` maps path separators to `-` and matches JsProvider path rules.
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
  // Match native behavior: map non-alphanumeric characters to '-'.
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

function isValidClaudeSessionMessage(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "user" && v.type !== "assistant") return false;
  if (!v.message || typeof v.message !== "object" || Array.isArray(v.message)) return false;
  var content = v.message.content;
  if (typeof content !== "string" && !Array.isArray(content)) return false;
  return true;
}

function validateEntry(v) {
  return isValidClaudeSessionDelta(v);
}

function isValidClaudeSessionDelta(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (!CLAUDE_SESSION_DELTA_TYPES[v.type]) return false;
  if (v.type === "system") {
    var sc = v.content;
    return typeof sc === "string" || Array.isArray(sc);
  }
  var msg = v.message;
  if (!msg || typeof msg !== "object" || Array.isArray(msg)) return false;
  if (typeof msg.content !== "string" && !Array.isArray(msg.content)) return false;
  return true;
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

// Task-file helpers: write to host temp files with provider-specific suffixes.
// Windows adds a UTF-8 BOM so PowerShell parses task bodies correctly.
function writeTaskTempFile(task) {
  var suffix = ".ct-task-" + randomShortHex(8);
  var win = host.platform() === "windows";
  // PowerShell reads UTF-8 reliably only with BOM; prepend it for Windows task files.
  var body = win ? "﻿" + task : task;
  return host.fs.writeTempFile(body, suffix);
}

function randomShortHex(len) {
  var hex = "0123456789abcdef";
  var out = "";
  for (var i = 0; i < len; i++) out += hex.charAt((Math.random() * 16) | 0);
  return out;
}

// Hook recognition helpers.
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

// ── /usage screen parsing (port of the native usage_tui module) ─────────

function usageNormalize(raw) {
  var out = "";
  for (var i = 0; i < raw.length; i++) {
    var ch = raw[i];
    if (ch === "\u001b" || ch === "\r") continue;
    if ("█▌▍▎▏▊▋".indexOf(ch) !== -1) { out += " "; continue; }
    out += ch;
  }
  return out;
}

function usageParseBlock(text, marker, isWeekly, nowMs) {
  var re = new RegExp(
    marker + "[\\s\\S]{0,800}?(\\d+(?:\\.\\d+)?)\\s*%\\s*used[\\s\\S]*?Resets?\\s*([^()\\n]+?)\\s*\\(",
  );
  var caps = re.exec(text);
  if (!caps) return { pct: null, resetsMs: null };
  var pct = parseFloat(caps[1]);
  if (isNaN(pct)) pct = null;
  var timeRaw = (caps[2] || "").trim();
  var resetsMs = isWeekly
    ? usageParseWeeklyReset(timeRaw, nowMs)
    : usageParseTodayClock(timeRaw, nowMs);
  return { pct: pct, resetsMs: resetsMs };
}

function usageParseHMmAmPm(s) {
  s = String(s || "").trim().toLowerCase();
  var pm;
  if (s.slice(-2) === "pm") pm = true;
  else if (s.slice(-2) === "am") pm = false;
  else return null;
  var stripped = s.slice(0, -2).trim();
  var hh, mm;
  var idx = stripped.indexOf(":");
  if (idx !== -1) {
    hh = parseInt(stripped.slice(0, idx).trim(), 10);
    mm = parseInt(stripped.slice(idx + 1).trim(), 10);
  } else {
    hh = parseInt(stripped, 10);
    mm = 0;
  }
  if (isNaN(hh) || isNaN(mm) || hh === 0 || hh > 12 || mm > 59) return null;
  return { hh: hh, mm: mm, pm: pm };
}

function usageParseTodayClock(s, nowMs) {
  var t = usageParseHMmAmPm(s);
  if (!t) return null;
  var hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  var now = new Date(nowMs);
  var cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, t.mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1000);
  return cand.getTime();
}

function usageParseWeeklyReset(s, nowMs) {
  return usageParseMonthDayClock(s, nowMs)
    ?? usageParseRelativeDayClock(s, nowMs)
    ?? usageParseTodayClock(s, nowMs);
}

var USAGE_MONTHS = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7,
  sep: 8, sept: 8, september: 8, oct: 9, october: 9, nov: 10, november: 10,
  dec: 11, december: 11,
};

function usageParseMonthDayClock(s, nowMs) {
  var parts = usageCleanParts(s);
  while (parts.length > 0 && usageIsWeekday(parts[0])) parts.shift();
  if (parts.length < 3) return null;
  var month = USAGE_MONTHS[parts[0]];
  if (month === undefined) return null;
  var day = usageParseDayToken(parts[1]);
  if (day === null) return null;
  var idx = 2;
  var explicitYear = null;
  if (/^\d{4}$/.test(parts[idx] || "")) {
    explicitYear = parseInt(parts[idx], 10);
    idx++;
  }
  if (parts[idx] === "at") idx++;
  var t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  var hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  var now = new Date(nowMs);
  var year = explicitYear ?? now.getFullYear();
  var cand = new Date(year, month, day, hour24, t.mm, 0, 0);
  if (explicitYear === null && cand.getTime() < nowMs) {
    cand = new Date(year + 1, month, day, hour24, t.mm, 0, 0);
  }
  return cand.getTime();
}

function usageParseRelativeDayClock(s, nowMs) {
  var parts = usageCleanParts(s);
  var first = parts[0];
  var offsetDays;
  if (first === "today" || first === "tonight") offsetDays = 0;
  else if (first === "tomorrow") offsetDays = 1;
  else return null;
  var idx = 1;
  if (parts[idx] === "at") idx++;
  var t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  var hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  var now = new Date(nowMs);
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + offsetDays,
    hour24,
    t.mm,
    0,
    0,
  ).getTime();
}

function usageCleanParts(s) {
  return String(s || "")
    .trim()
    .toLowerCase()
    .replace(/,/g, " ")
    .split(/\s+/)
    .map(function (p) { return p.replace(/[.;]+$/g, ""); })
    .filter(Boolean);
}

function usageParseDayToken(s) {
  var n = parseInt(String(s || "").replace(/(?:st|nd|rd|th)$/i, ""), 10);
  return Number.isFinite(n) && n >= 1 && n <= 31 ? n : null;
}

function usageIsWeekday(s) {
  return /^(mon|monday|tue|tues|tuesday|wed|wednesday|thu|thur|thurs|thursday|fri|friday|sat|saturday|sun|sunday)$/.test(s);
}
