// Cursor Agent CLI provider plugin.
//
// Cursor ships a terminal coding agent as `cursor-agent` (curl
// https://cursor.com/install -fsSL | bash). Reference:
//   https://cursor.com/docs/cli/overview
//   https://cursor.com/docs/cli/reference/parameters
//
// This plugin is drop-in third-party: lives in ~/.codeterm/plugins/,
// gets discovered by the watcher, registered without any Rust changes.

var TITLE_RE = /cursor[- ]?agent|cursor/i;

// Cursor TUI banner + typical chrome. Two-part fingerprint isn't
// strictly needed — "Cursor Agent" + the prompt char are distinctive
// enough that no other provider mints them together.
var OUTPUT_FINGERPRINTS = [
  "Cursor Agent",
  "cursor-agent",
  "(cursor)",
];

var TUI_FRAGMENTS = ["cursor-agent", "/model", "/help"];

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
        c !== "─" && c !== "━" && c !== "═" && c !== "█" && c !== "▀" &&
        c !== " " && c !== "\t" && c !== "\n" && c !== "\r"
      ) {
        return false;
      }
    }
    return true;
  },

  hasEventMarkers: function (chunk) {
    var c = String(chunk || "");
    // Cursor asks "Do you want to allow this command?" before each
    // shell action when not in --force mode.
    return c.indexOf("allow this") !== -1
      || c.indexOf("approve") !== -1
      || c.indexOf("Run this command") !== -1;
  },

  detectEvents: function (text) {
    var t = String(text || "");
    var events = [];
    if (
      t.indexOf("Do you want to allow") !== -1 ||
      t.indexOf("Run this command?") !== -1 ||
      t.indexOf("Approve this") !== -1
    ) {
      events.push("permission_request");
    }
    return events;
  },

  // Launch:
  //   interactive TUI:  cursor-agent [--force]
  //   headless one-shot: cursor-agent -p --output-format stream-json "<task>"
  //
  // --force = the published flag for "skip approvals" (cursor's
  // equivalent of claude's --dangerously-skip-permissions).
  buildLaunchCommand: function (params) {
    var p = params || {};
    var parts = ["cursor-agent"];
    if (p.headless && p.task) {
      parts.push("-p");
      parts.push("--output-format");
      parts.push("stream-json");
      if (p.skipPermissions) parts.push("--force");
      parts.push(host.shell.quote(p.task));
    } else {
      if (p.skipPermissions) parts.push("--force");
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(p.args[i]);
    }
    return parts.join(" ");
  },

  // `cursor-agent --resume <id>` resumes a saved thread.
  buildResumeCommand: function (sessionId, skipPermissions) {
    var parts = ["cursor-agent"];
    if (skipPermissions) parts.push("--force");
    parts.push("--resume");
    parts.push(host.shell.quote(sessionId));
    return parts.join(" ");
  },

  // ── Session detection (~/.cursor/chats SQLite) ─────────────────────
  //
  // Cursor stores chats per-workspace at
  //   ~/.cursor/chats/<md5(cwd)>/<agentId>/store.db
  // where `meta.value WHERE key='0'` is a TEXT cell containing
  // HEX-ENCODED JSON with fields { agentId, name, lastUsedModel,
  // createdAt, ... }. Same store is shared by the IDE and the
  // cursor-agent CLI — so even without the CLI binary on PATH, panes
  // opened in cwd's that the IDE has already chatted about light up
  // with a real session id + model.

  detectSessionId: function (cwd, exclude, maxAgeMs) {
    var rows = cursorScanWorkspaceChats(cwd);
    if (!rows.length) return null;
    var nowMs = host.unixNowMs();
    var cutoff = maxAgeMs > 0 ? nowMs - maxAgeMs : null;
    var excluded = exclude || [];
    for (var i = 0; i < rows.length; i++) {
      var r = rows[i];
      if (excluded.indexOf(r.agentId) !== -1) continue;
      if (cutoff !== null && r.createdAt < cutoff) continue;
      return r.agentId;
    }
    return null;
  },

  sessionExists: function (cwd, sessionId) {
    var rows = cursorScanWorkspaceChats(cwd);
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].agentId === sessionId) return true;
    }
    return false;
  },

  detectSessionModel: function (cwd, sessionId) {
    var rows = cursorScanWorkspaceChats(cwd);
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].agentId === sessionId) {
        return cursorShortModelLabel(rows[i].lastUsedModel);
      }
    }
    return null;
  },
};

// ── Helpers ──────────────────────────────────────────────────────────

// Scan ~/.cursor/chats/<md5(cwd)>/*/store.db, return [{agentId,
// lastUsedModel, createdAt}] newest-first. Cursor IDE hashes the
// backslash native-Windows form of the cwd ("C:\\Users\\..."), but
// codeterm passes pane cwds in forward-slash form
// ("C:/Users/..."). md5 differs, so the bucket lookup misses. Try
// every plausible variant and merge — also covers cases where one
// session was started from PowerShell and another from MSYS bash.
function cursorScanWorkspaceChats(cwd) {
  var base = host.expandHome("~/.cursor/chats");
  if (!base) return [];
  var sep = host.platform() === "windows" ? "\\" : "/";
  var s = String(cwd || "");
  var variants = [s, s.replace(/\//g, "\\"), s.replace(/\\/g, "/")];
  var seenBuckets = {};
  var out = [];
  for (var v = 0; v < variants.length; v++) {
    var hash = host.md5(variants[v]);
    if (seenBuckets[hash]) continue;
    seenBuckets[hash] = true;
    var wsDir = base + sep + hash;
    if (!host.fs.fileExists(wsDir)) continue;
    var entries;
    try { entries = host.fs.readDir(wsDir); } catch (_) { continue; }
    for (var i = 0; i < entries.length; i++) {
      var e = entries[i];
      if (!e || !e.isDir) continue;
      var db = wsDir + sep + e.name + sep + "store.db";
      if (!host.fs.fileExists(db)) continue;
      var rows;
      try {
        rows = host.fs.sqliteQuery(db, "SELECT value FROM meta WHERE key='0' LIMIT 1", []);
      } catch (_) { continue; }
      if (!rows || !rows.length) continue;
      var raw = rows[0].value;
      if (typeof raw !== "string" || !raw) continue;
      var json = hexToUtf8(raw);
      if (!json) continue;
      var meta;
      try { meta = JSON.parse(json); } catch (_) { continue; }
      if (!meta || typeof meta.agentId !== "string") continue;
      out.push({
        agentId: meta.agentId,
        lastUsedModel: typeof meta.lastUsedModel === "string" ? meta.lastUsedModel : null,
        createdAt: typeof meta.createdAt === "number" ? meta.createdAt : 0,
      });
    }
  }
  out.sort(function (a, b) { return b.createdAt - a.createdAt; });
  return out;
}

// Decode lowercase hex string → UTF-8 string. Cursor stores the meta
// blob as TEXT-hex inside a sqlite TEXT cell.
function hexToUtf8(hex) {
  if (hex.length % 2 !== 0) return null;
  var bytes = [];
  for (var i = 0; i < hex.length; i += 2) {
    var b = parseInt(hex.substr(i, 2), 16);
    if (isNaN(b)) return null;
    bytes.push(b);
  }
  // Decode UTF-8 by hand (QuickJS has no TextDecoder).
  var s = "";
  var i = 0;
  while (i < bytes.length) {
    var b = bytes[i++];
    if (b < 0x80) { s += String.fromCharCode(b); }
    else if (b < 0xc0) { s += "?"; }
    else if (b < 0xe0) {
      s += String.fromCharCode(((b & 0x1f) << 6) | (bytes[i++] & 0x3f));
    } else if (b < 0xf0) {
      s += String.fromCharCode(((b & 0x0f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f));
    } else {
      // 4-byte → surrogate pair
      var cp = ((b & 0x07) << 18) | ((bytes[i++] & 0x3f) << 12)
             | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f);
      cp -= 0x10000;
      s += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
    }
  }
  return s;
}

// Cursor's `lastUsedModel` strings: "default" (Auto), "gpt-5",
// "claude-4-sonnet", etc. Short-label so the tab badge stays readable.
function cursorShortModelLabel(id) {
  if (!id) return null;
  var m = String(id).toLowerCase();
  if (m === "default" || m === "auto") return "Auto";
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
}
