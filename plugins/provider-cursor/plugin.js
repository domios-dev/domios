// Auto-generated from src/index.ts — keep in sync.
// Loaded by codeterm's embedded QuickJS runtime. No imports allowed;
// everything must be a single self-contained CommonJS file.

function shellQuote(s) {
  return "'" + String(s).replace(/'/g, "'\\''") + "'";
}

var TITLE_RE = /\b(cursor[- ]?agent|cursor cli)\b/i;

var OUTPUT_FINGERPRINTS = [
  "Cursor Agent",
  "/plan",
  "/ask",
  "Max Mode",
];

var NOISE_FRAGMENTS = [
  "ctrl+c to exit",
  "loading...",
  "thinking...",
];

module.exports.default = {
  detectFromTitle: function (title) {
    return TITLE_RE.test(title || "");
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
    return (
      lower.indexOf("max mode") !== -1 ||
      lower.indexOf("/plan") !== -1 ||
      lower.indexOf("/ask") !== -1
    );
  },

  isOutputNoise: function (text) {
    var lower = String(text || "").toLowerCase();
    for (var i = 0; i < NOISE_FRAGMENTS.length; i++) {
      if (lower.indexOf(NOISE_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
  },

  hasEventMarkers: function (chunk) {
    var c = String(chunk || "");
    return (
      c.indexOf("❯") !== -1 ||
      (c.indexOf("1.") !== -1 && c.indexOf("2.") !== -1) ||
      c.toLowerCase().indexOf("allow this command") !== -1
    );
  },

  buildLaunchCommand: function (params) {
    var p = params || {};
    var parts = ["agent"];
    if (p.headless && p.task) {
      parts.push("-p");
      parts.push(shellQuote(p.task));
      parts.push("--output-format");
      parts.push("text");
    }
    if (p.skipPermissions) {
      parts.push("--sandbox");
      parts.push("disabled");
    }
    if (p.args && p.args.length > 0) {
      for (var i = 0; i < p.args.length; i++) parts.push(shellQuote(p.args[i]));
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
    var parts = ["agent", "--resume", shellQuote(sessionId)];
    if (skipPermissions) {
      parts.push("--sandbox");
      parts.push("disabled");
    }
    return parts.join(" ");
  },

  parsePrompt: function (text) {
    var lines = String(text || "").split("\n");
    var qLine = "";
    var opts = [];
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].trim();
      var m = line.match(/^(\d+)\.\s+(.+)$/);
      if (m) {
        opts.push([m[1], m[2].trim()]);
      } else if (line.endsWith("?") && !qLine) {
        qLine = line;
      }
    }
    if (opts.length < 2 || !qLine) return null;
    return { question: qLine, options: opts };
  },
};
