// OpenCode (sst/opencode) provider plugin — pure-logic surface.
// Session storage is a sqlite database + per-part JSON files (modern
// builds keep everything in opencode.db). Enumeration and parsing
// those stay in the native Rust provider; this plugin covers detect,
// schema, launch/resume command building, slash commands.

function shellQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'";
}

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
    var p = params || {};
    var parts = ["opencode"];
    if (p.headless && p.task) {
      parts = ["opencode", "run"];
      if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
      parts.push("--format");
      parts.push("json");
      parts.push(shellQuote(p.task));
    } else if (p.skipPermissions) {
      parts.push("--dangerously-skip-permissions");
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
    var parts = ["opencode", "--session", shellQuote(sessionId)];
    return parts.join(" ");
  },
};
