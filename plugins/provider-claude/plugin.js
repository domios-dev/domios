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

function shellQuote(s) {
  // Cross-platform shim: single-quote for POSIX, the Rust-side
  // `ShellSyntax::quote` does platform-specific quoting natively.
  // Plugins emit POSIX form; if the user opts-in on Windows they'll
  // see PowerShell forward through (claude on Windows runs under
  // pwsh.exe which accepts POSIX single quotes literally).
  return "'" + String(s).replace(/'/g, "'\\''") + "'";
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

  // Simple launch — Rust native provider handles stdin-pipe tasks +
  // per-OS env quoting. JS form is enough for the interactive launch
  // path; opt-in via CODETERM_JS_PROVIDERS=claude to use it.
  buildLaunchCommand: function (params) {
    var p = params || {};
    var parts = ["claude"];
    if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
    if (p.task && !p.headless) {
      parts.push(shellQuote(p.task));
    }
    if (p.agentName) {
      parts.push("--agent");
      parts.push(shellQuote(p.agentName));
    }
    if (p.sessionId && !p.headless) {
      parts.push("--session-id");
      parts.push(shellQuote(p.sessionId));
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(p.args[i]);
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    var parts = ["claude"];
    if (skipPermissions) parts.push("--dangerously-skip-permissions");
    parts.push("--resume");
    parts.push(shellQuote(sessionId));
    return parts.join(" ");
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
      // Rust SessionMessage uses snake_case field names — return is_meta,
      // not isMeta, or serde will default it to false on deserialize.
      out.push({ role: msgType, text: text2, ts: ts, is_meta: isMeta });
    }
    return out;
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
};
