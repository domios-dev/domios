// Codex provider plugin — pure-logic surface. Session file enumeration
// + rollout JSONL fan-out stay in the native Rust provider because
// codex stores sessions in a YYYY/MM/DD tree that needs recursive
// directory walking (cheaper in Rust than JSON-serializing per file).

function shellQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'";
}

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
      parts.push(shellQuote(p.task));
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
    // session_id should be a UUID; native Rust extracts it from
    // rollout filename stems when needed. JS plugin assumes UUID.
    parts.push(shellQuote(sessionId));
    return parts.join(" ");
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
      out.push({ role: role, text: text2, ts: ts, is_meta: false });
    }
    return out;
  },
};
