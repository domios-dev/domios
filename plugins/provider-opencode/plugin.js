// OpenCode (sst/opencode) provider plugin. Detection + slash commands +
// schema + sqlite-backed session detection via host.fs.sqliteQuery
// against `<XDG_DATA_HOME|~/.local/share>/opencode/opencode.db`.

var TITLE_RE = /opencode/i;
// Two-part fingerprint: must mention opencode AND show a TUI / prompt
// signal. Single mention isn't enough (a shell command typing
// "opencode" would otherwise misfire); single signal isn't enough
// (claude has "Permission required" too).
var OUTPUT_MARKERS = ["opencode", "OpenCode", "OpenCode Zen"];
var OUTPUT_SIGNALS = [
  "Permission required",
  "Allow this tool call",
  "Approve this command",
  "█▀▀█",
  "Build · ",
  "┃",
  "╹",
];
var TUI_FRAGMENTS = ["tab agents", "ctrl+p commands"];

module.exports.default = {
  detectFromTitle: function (title) {
    return TITLE_RE.test(String(title || ""));
  },

  detectFromOutput: function (text) {
    var t = String(text || "");
    var hasMarker = false;
    for (var i = 0; i < OUTPUT_MARKERS.length; i++) {
      if (t.indexOf(OUTPUT_MARKERS[i]) !== -1) { hasMarker = true; break; }
    }
    if (!hasMarker) return false;
    for (var j = 0; j < OUTPUT_SIGNALS.length; j++) {
      if (t.indexOf(OUTPUT_SIGNALS[j]) !== -1) return true;
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
    return c.indexOf("Permission required") !== -1 || c.indexOf("approve") !== -1;
  },

  detectEvents: function (text) {
    var t = String(text || "");
    var events = [];
    if (
      t.indexOf("Permission required") !== -1 ||
      t.indexOf("Allow this tool call") !== -1 ||
      t.indexOf("Approve this command") !== -1
    ) {
      events.push("permission_request");
    }
    return events;
  },

  buildLaunchCommand: function (params) {
    // `--dangerously-skip-permissions` is `run`-only — opencode TUI
    // rejects unknown args and exits to help. Drop it silently for
    // interactive launches; honour caller intent in headless+task.
    var p = params || {};
    var parts = ["opencode"];
    if (p.headless && p.task) {
      parts = ["opencode", "run"];
      if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
      parts.push("--format");
      parts.push("json");
      parts.push(host.shell.quote(p.task));
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(p.args[i]);
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    // OpenCode resume goes through the TUI — --dangerously-skip-permissions
    // not supported on resume. Per-session permission rules live on the
    // session JSON. Honour intent by silently dropping the flag.
    var _ = skipPermissions;
    var parts = ["opencode", "--session", host.shell.quote(sessionId)];
    return parts.join(" ");
  },

  // ── Sqlite-backed session detection (opencode.db) ───────────────────
  //
  // Modern opencode keeps sessions in `<XDG_DATA_HOME|~/.local/share>/
  // opencode/opencode.db`. Each session row has `directory` (cwd it
  // started in, normalised to native path separators), `time_created`
  // ms epoch, and `time_archived` (NULL when live). We surface only
  // live sessions for the requested cwd, newest-first.

  detectSessionId: function (cwd, exclude, maxAgeMs) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    var rows = host.fs.sqliteQuery(
      db,
      "SELECT id, time_created FROM session " +
        "WHERE (directory = ?1 OR directory = ?2) AND time_archived IS NULL " +
        "ORDER BY time_created DESC LIMIT 50",
      [cwd.replace(/\\/g, "/"), cwd.replace(/\//g, "\\")]
    );
    var nowMs = host.unixNowMs();
    var cutoff = maxAgeMs > 0 ? nowMs - maxAgeMs : null;
    var excluded = exclude || [];
    for (var i = 0; i < rows.length; i++) {
      var id = rows[i].id;
      if (excluded.indexOf(id) !== -1) continue;
      if (cutoff !== null && rows[i].time_created < cutoff) continue;
      return id;
    }
    return null;
  },

  sessionExists: function (_cwd, sessionId) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return false;
    var rows = host.fs.sqliteQuery(
      db,
      "SELECT 1 AS one FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    return rows.length > 0;
  },

  detectActiveSession: function (cwd, _currentSid) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    var rows = host.fs.sqliteQuery(
      db,
      "SELECT m.session_id AS session_id FROM message m " +
        "JOIN session s ON s.id = m.session_id " +
        "WHERE (s.directory = ?1 OR s.directory = ?2) AND s.time_archived IS NULL " +
        "ORDER BY m.time_created DESC LIMIT 1",
      [cwd.replace(/\\/g, "/"), cwd.replace(/\//g, "\\")]
    );
    return rows.length > 0 ? rows[0].session_id : null;
  },

  detectSessionModel: function (cwd, sessionId) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    // Prefer the most recent assistant model for the explicit session;
    // fall back to the latest assistant model in the cwd when the
    // session hasn't produced an assistant turn yet (post-/model swap).
    if (sessionId) {
      var model = scanMessagesForModel(db,
        "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 20",
        [sessionId]);
      if (model) return model;
    }
    return scanMessagesForModel(db,
      "SELECT m.data AS data FROM message m " +
        "JOIN session s ON s.id = m.session_id " +
        "WHERE (s.directory = ?1 OR s.directory = ?2) " +
        "ORDER BY m.time_created DESC LIMIT 50",
      [cwd.replace(/\\/g, "/"), cwd.replace(/\//g, "\\")]);
  },
};

function opencodeDbPath() {
  var base = host.envGet("XDG_DATA_HOME");
  if (!base) base = host.expandHome("~/.local/share");
  if (!base) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return base + sep + "opencode" + sep + "opencode.db";
}

function scanMessagesForModel(db, sql, params) {
  var rows = host.fs.sqliteQuery(db, sql, params);
  for (var i = 0; i < rows.length; i++) {
    var raw = rows[i].data;
    if (!raw) continue;
    try {
      var v = JSON.parse(raw);
      if (v && v.role === "assistant" && typeof v.modelID === "string" && v.modelID) {
        return v.modelID;
      }
    } catch (_) {
      // skip malformed row
    }
  }
  return null;
}
