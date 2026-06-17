// OpenCode (sst/opencode) provider: detect, slash commands, schema.

var CODETERM_OPENCODE_PLUGIN_MARKER = "codeterm-opencode-session-binding-v2";
var CODETERM_OPENCODE_PLUGIN_FILE = "codeterm-session-binding.js";
var TITLE_RE = /opencode/i;
// Two-part fingerprint: opencode mention + TUI/prompt signal to avoid false positives.
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

  // Parse opencode's permission box. Its options are a single horizontal row
  // ("Allow once  Allow always  Reject") rather than a numbered list, so the
  // host's universal picker can't read it — the plugin owns its own format.
  // Returns { question, options: [[label, value]] } with value = 1-based index,
  // or null. The host navigates to the option via build_prompt_response(index).
  parsePrompt: function (text) {
    // Strip box borders / decorations so labels and the question stay clean
    // regardless of how opencode frames the dialog.
    function clean(line) {
      return String(line)
        .replace(/[│┃|╎╏┆┊]/g, " ")
        .replace(/^[\s←⚠»·]+/, "")
        .replace(/[\s·»]+$/, "")
        .trim();
    }
    var lines = String(text || "").split("\n").map(clean);
    var permIdx = -1;
    for (var i = lines.length - 1; i >= 0; i--) {
      if (lines[i].indexOf("Permission required") !== -1) { permIdx = i; break; }
    }
    if (permIdx < 0) return null;

    // Options row: the line with the action words below the prompt.
    var optLine = null;
    for (var j = permIdx; j < lines.length; j++) {
      var lc = lines[j].toLowerCase();
      if (
        (lc.indexOf("allow") !== -1 || lc.indexOf("reject") !== -1 || lc.indexOf("deny") !== -1) &&
        lc.indexOf("permission required") === -1
      ) { optLine = lines[j]; break; }
    }
    if (!optLine) return null;
    var labels = optLine.split(/\s{2,}/).map(function (s) { return s.trim(); }).filter(Boolean);
    if (labels.length < 2) return null;
    var options = labels.map(function (label, idx) { return [label, String(idx + 1)]; });

    // Question: first meaningful line after the prompt, skipping the
    // "Patterns" label and blank/decoration-only lines.
    var question = "";
    for (var k = permIdx + 1; k < lines.length; k++) {
      var q = lines[k];
      if (q && q.toLowerCase() !== "patterns") { question = q; break; }
    }
    if (!question) question = "Permission required";
    return { question: question, options: options };
  },

  buildLaunchCommand: function (params) {
    var p = params || {};
    var parts = ["opencode"];
    if (p.args && p.args.length > 0) {
      // Quote each arg — caller-supplied via the spawn API; prevents shell injection.
      for (var i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    return parts.join(" ");
  },

  buildResumeCommand: function (sessionId, skipPermissions) {
  // Resume uses the normal TUI path; permission rules come from saved session config.
    var _ = skipPermissions;
    var parts = ["opencode", "--session", host.shell.quote(sessionId)];
    return parts.join(" ");
  },

  detectSessionId: function (cwd, exclude, maxAgeMs) {
    return null;
  },

  checkIntegration: function () {
    var cfg = opencodeConfigPath();
    var plugin = opencodeBindingPluginPath();
    var pluginExists = !!plugin && host.fs.fileExists(plugin);
    var pluginBody = pluginExists ? String(host.fs.readFile(plugin) || "") : "";
    // Require the binding-nonce header too; a pre-nonce plugin file is outdated → reinstall.
    var pluginCurrent = pluginExists &&
      pluginBody.indexOf(CODETERM_OPENCODE_PLUGIN_MARKER) !== -1 &&
      pluginBody.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1;
    if (!cfg || !host.fs.fileExists(cfg)) {
      return {
        config_file_exists: false,
        plugin_file_exists: pluginExists,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
      };
    }
    var json = host.fs.readJson(cfg);
    if (!json) {
      return {
        config_file_exists: true,
        plugin_file_exists: pluginExists,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
      };
    }
    var target = opencodePluginEntry();
    var entries = json.plugin;
    var installed = Array.isArray(entries) && entries.some(function (entry) {
      return opencodePluginEntryMatches(entry, target);
    });
    return {
      config_file_exists: true,
      plugin_file_exists: pluginExists,
      plugin_file_current: pluginCurrent,
      plugin_installed: installed,
    };
  },

  installIntegration: function (apiPort) {
    var cfg = opencodeConfigPath();
    var plugin = opencodeBindingPluginPath();
    var target = opencodePluginEntry();
    if (!cfg || !plugin || !target) return { ok: false, error: "could not resolve opencode config path" };

    var json = host.fs.fileExists(cfg)
      ? (host.fs.readJson(cfg) || { "$schema": "https://opencode.ai/config.json" })
      : { "$schema": "https://opencode.ai/config.json" };
    if (!json || typeof json !== "object" || Array.isArray(json)) {
      return { ok: false, error: "opencode.json root must be an object" };
    }
    if (!Array.isArray(json.plugin)) json.plugin = [];
    json.plugin = json.plugin.filter(function (entry) {
      return !opencodePluginEntryMatches(entry, target);
    });
    json.plugin.push(target);

    if (!host.fs.writeFile(plugin, opencodeBindingPluginSource(apiPort | 0))) {
      return { ok: false, error: "writeFile failed for " + plugin };
    }
    if (!host.fs.writeJson(cfg, json)) {
      return { ok: false, error: "writeJson failed for " + cfg };
    }
    host.log("info", "[opencode-plugin] session binding plugin installed at " + plugin);
    return { ok: true };
  },

  uninstallIntegration: function () {
    var cfg = opencodeConfigPath();
    var plugin = opencodeBindingPluginPath();
    var target = opencodePluginEntry();
    if (cfg && host.fs.fileExists(cfg)) {
      var json = host.fs.readJson(cfg);
      if (json && typeof json === "object" && Array.isArray(json.plugin)) {
        json.plugin = json.plugin.filter(function (entry) {
          return !opencodePluginEntryMatches(entry, target);
        });
        if (!host.fs.writeJson(cfg, json)) {
          return { ok: false, error: "writeJson failed for " + cfg };
        }
      }
    }
    if (plugin && host.fs.fileExists(plugin) && !host.fs.removeFile(plugin)) {
      return { ok: false, error: "removeFile failed for " + plugin };
    }
    return { ok: true };
  },

  ensureIntegrationOnStartup: function (apiPort) {
    var status = this.checkIntegration();
    if (status && status.plugin_installed && status.plugin_file_exists && status.plugin_file_current) return { ok: true };
    return this.installIntegration(apiPort);
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

  // Session creation time (unix ms). Lets the host apply its launch-window
  // birth-time guard to sqlite-backed sessions, which have no per-session file.
  sessionCreatedMs: function (_cwd, sessionId) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    var rows = host.fs.sqliteQuery(
      db,
      "SELECT time_created FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    if (!rows.length) return null;
    var tc = rows[0].time_created;
    return typeof tc === "number" ? tc : null;
  },

  detectActiveSession: function (cwd, _currentSid) {
    return null;
  },

  detectSessionModel: function (cwd, sessionId) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
  // Prefer explicit session model; fall back to latest model in cwd.
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

  // opencode keeps chat in opencode.db (part JOIN message), not a JSONL the
  // host can tail — so the chat UI reads it structured. cursor is the max
  // part.time_created seen, passed back to fetch only newer parts next poll.
  usesStructuredChat: function () { return true; },

  readStructuredChat: function (_cwd, sessionId, cursor) {
    var db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return { messages: [], cursor: cursor || null };
    var sinceMs = cursor ? (parseInt(cursor, 10) || 0) : 0;
    var rows = host.fs.sqliteQuery(
      db,
      "SELECT p.id AS id, p.time_created AS ts, p.data AS part, m.data AS msg " +
        "FROM part p JOIN message m ON m.id = p.message_id " +
        "WHERE p.session_id = ?1 AND p.time_created >= ?2 " +
        "ORDER BY p.time_created ASC, p.id ASC",
      [sessionId, sinceMs]
    );
    var messages = [];
    var maxTs = sinceMs;
    for (var i = 0; i < rows.length; i++) {
      var ts = rows[i].ts || 0;
      if (ts > maxTs) maxTs = ts;
      var part = safeJson(rows[i].part);
      if (!part) continue;
      var role = "assistant";
      var msg = safeJson(rows[i].msg);
      if (msg && msg.role) role = msg.role;
      var cm = partToChatMsg(part, role, String(rows[i].id), ts);
      if (cm) messages.push(cm);
    }
    return { messages: messages, cursor: String(maxTs) };
  },
};

function safeJson(s) {
  try { return JSON.parse(s); } catch (_) { return null; }
}

// Map one opencode `part` blob to a ChatMsg (camelCase, matching the Rust
// serde shape). text→user/assistant, reasoning→thinking, tool→tool_call.
function partToChatMsg(part, role, id, tsMs) {
  var timestamp = String(tsMs);
  var type = part && part.type;
  if (type === "text") {
    var txt = (part.text || "");
    if (!txt.trim()) return null;
    return { id: id, type: role === "user" ? "user" : "assistant", content: txt, timestamp: timestamp };
  }
  if (type === "reasoning") {
    var rtxt = (part.text || "");
    if (!rtxt.trim()) return null;
    return { id: id, type: "thinking", content: rtxt, timestamp: timestamp };
  }
  if (type === "tool") {
    var tool = part.tool || "tool";
    var state = part.state || {};
    var out = { id: id, type: "tool_call", content: "", timestamp: timestamp, toolName: tool };
    if (state.input !== undefined) out.toolInput = state.input;
    if (typeof state.output === "string") out.toolResult = state.output;
    return out;
  }
  return null;
}

function opencodeDbPath() {
  var base = host.envGet("XDG_DATA_HOME");
  if (!base) base = host.expandHome("~/.local/share");
  if (!base) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return base + sep + "opencode" + sep + "opencode.db";
}

function opencodeConfigDir() {
  var override = host.envGet("OPENCODE_CONFIG_DIR");
  if (override) return override;
  return host.expandHome("~/.config/opencode");
}

function opencodeConfigPath() {
  var dir = opencodeConfigDir();
  if (!dir) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + "opencode.json";
}

function opencodeBindingPluginPath() {
  var dir = opencodeConfigDir();
  if (!dir) return null;
  var sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + CODETERM_OPENCODE_PLUGIN_FILE;
}

function opencodePluginEntry() {
  var path = opencodeBindingPluginPath();
  return path ? path.replace(/\\/g, "/") : null;
}

function opencodePluginEntryMatches(entry, target) {
  if (!target) return false;
  var value = null;
  if (typeof entry === "string") value = entry;
  else if (Array.isArray(entry) && typeof entry[0] === "string") value = entry[0];
  if (!value) return false;
  return value.replace(/\\/g, "/") === target;
}

function opencodeBindingPluginSource(apiPort) {
  return [
    "// " + CODETERM_OPENCODE_PLUGIN_MARKER,
    "const PROVIDER_ID = \"opencode\";",
    "const FALLBACK_PORT = \"" + String(apiPort || 7686) + "\";",
    "",
    "function endpoint() {",
    "  const pane = process.env.CODETERM_PANE_ID;",
    "  if (!pane) return null;",
    "  const base =",
    "    process.env.CODETERM_API_URL ||",
    "    `http://127.0.0.1:${process.env.CODETERM_API_PORT || FALLBACK_PORT}`;",
    "  return `${base}/api/panes/${encodeURIComponent(pane)}/hook/session_start`;",
    "}",
    "",
    "function normalizeSessionID(value) {",
    "  return typeof value === \"string\" && value.startsWith(\"ses_\") ? value : null;",
    "}",
    "",
    "function firstSessionID(...values) {",
    "  for (const value of values) {",
    "    const sessionID = normalizeSessionID(value);",
    "    if (sessionID) return sessionID;",
    "  }",
    "  return null;",
    "}",
    "",
    "async function report(sessionID, source, directory) {",
    "  sessionID = normalizeSessionID(sessionID);",
    "  if (!sessionID) return;",
    "  const url = endpoint();",
    "  if (!url) return;",
    "  const headers = {",
    "    \"content-type\": \"application/json\",",
    "    \"x-codeterm-agent-provider\": PROVIDER_ID,",
    "  };",
    "  if (process.env.CODETERM_SESSION_BINDING_NONCE) {",
    "    headers[\"x-codeterm-binding-nonce\"] = process.env.CODETERM_SESSION_BINDING_NONCE;",
    "  }",
    "  if (process.env.CODETERM_API_TOKEN) {",
    "    headers.authorization = `Bearer ${process.env.CODETERM_API_TOKEN}`;",
    "  }",
    "  await fetch(url, {",
    "    method: \"POST\",",
    "    headers,",
    "    body: JSON.stringify({",
    "      provider_id: PROVIDER_ID,",
    "      session_id: sessionID,",
    "      source,",
    "      cwd: directory || process.cwd(),",
    "    }),",
    "  }).catch(() => {});",
    "}",
    "",
    "function eventSessionID(event) {",
    "  const p = event?.properties || {};",
    "  return firstSessionID(",
    "    p.sessionID,",
    "    p.session_id,",
    "    p.session?.id,",
    "    p.info?.sessionID,",
    "    p.info?.session_id,",
    "    p.message?.sessionID,",
    "    p.message?.session_id,",
    "    p.part?.sessionID,",
    "    p.part?.session_id,",
    "    event?.sessionID,",
    "    event?.session_id,",
    "    p.info?.id",
    "  );",
    "}",
    "",
    "function eventDirectory(event, fallback) {",
    "  return (",
    "    event?.properties?.info?.directory ||",
    "    event?.properties?.session?.directory ||",
    "    event?.properties?.directory ||",
    "    fallback",
    "  );",
    "}",
    "",
    "export default async function CodeTermSessionBindingPlugin({ directory }) {",
    "  return {",
    "    async event({ event }) {",
    "      if (",
    "        event?.type === \"session.created\" ||",
    "        event?.type === \"session.updated\" ||",
    "        event?.type === \"tui.session.select\" ||",
    "        event?.type === \"message.updated\" ||",
    "        event?.type === \"message.part.updated\"",
    "      ) {",
    "        await report(eventSessionID(event), event.type, eventDirectory(event, directory));",
    "      }",
    "    },",
    "    async \"chat.message\"(input) {",
    "      await report(input?.sessionID, \"chat.message\", directory);",
    "    },",
    "    async \"tool.execute.before\"(input) {",
    "      await report(input?.sessionID, \"tool.execute.before\", directory);",
    "    },",
    "    async \"shell.env\"(input, output) {",
    "      await report(input?.sessionID, \"shell.env\", input?.cwd || directory);",
    "      return output;",
    "    },",
    "  };",
    "}",
    "",
  ].join("\n");
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
