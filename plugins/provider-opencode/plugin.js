"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/provider-opencode/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
var CODETERM_OPENCODE_PLUGIN_MARKER = "codeterm-opencode-session-binding-v2";
var CODETERM_OPENCODE_PLUGIN_FILE = "codeterm-session-binding.js";
var TITLE_RE = /opencode/i;
var OUTPUT_MARKERS = ["opencode", "OpenCode", "OpenCode Zen"];
var OUTPUT_SIGNALS = [
  "Permission required",
  "Allow this tool call",
  "Approve this command",
  "\u2588\u2580\u2580\u2588",
  "Build \xB7 ",
  "\u2503",
  "\u2579"
];
var TUI_FRAGMENTS = ["tab agents", "ctrl+p commands"];
var plugin = {
  detectFromTitle(title) {
    return TITLE_RE.test(String(title || ""));
  },
  detectFromOutput(text) {
    const t = String(text || "");
    let hasMarker = false;
    for (let i = 0; i < OUTPUT_MARKERS.length; i++) {
      if (t.indexOf(OUTPUT_MARKERS[i]) !== -1) {
        hasMarker = true;
        break;
      }
    }
    if (!hasMarker) return false;
    for (let j = 0; j < OUTPUT_SIGNALS.length; j++) {
      if (t.indexOf(OUTPUT_SIGNALS[j]) !== -1) return true;
    }
    return false;
  },
  screenHasTui(screen) {
    const lower = String(screen || "").toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return false;
  },
  isOutputNoise(text) {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== "\u2588" && c !== "\u2580" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  hasEventMarkers(chunk) {
    const c = String(chunk || "");
    return c.indexOf("Permission required") !== -1 || c.indexOf("approve") !== -1;
  },
  detectEvents(text) {
    const t = String(text || "");
    const events = [];
    if (t.indexOf("Permission required") !== -1 || t.indexOf("Allow this tool call") !== -1 || t.indexOf("Approve this command") !== -1) {
      events.push("permission_request");
    }
    return events;
  },
  // Parse opencode's permission box. Its options are a single horizontal row
  // ("Allow once  Allow always  Reject") rather than a numbered list, so the
  // host's universal picker can't read it — the plugin owns its own format.
  // Returns { question, options: [[label, value]] } with value = 1-based index,
  // or null. The host navigates to the option via build_prompt_response(index).
  parsePrompt(text) {
    function clean(line) {
      return String(line).replace(/[│┃|╎╏┆┊]/g, " ").replace(/^[\s←⚠»·]+/, "").replace(/[\s·»]+$/, "").trim();
    }
    const lines = String(text || "").split("\n").map(clean);
    let permIdx = -1;
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].indexOf("Permission required") !== -1) {
        permIdx = i;
        break;
      }
    }
    if (permIdx < 0) return null;
    let optLine = null;
    for (let j = permIdx; j < lines.length; j++) {
      const lc = lines[j].toLowerCase();
      if ((lc.indexOf("allow") !== -1 || lc.indexOf("reject") !== -1 || lc.indexOf("deny") !== -1) && lc.indexOf("permission required") === -1) {
        optLine = lines[j];
        break;
      }
    }
    if (!optLine) return null;
    const labels = optLine.split(/\s{2,}/).map((s) => s.trim()).filter(Boolean);
    if (labels.length < 2) return null;
    const options = labels.map((label, idx) => [label, String(idx + 1)]);
    let question = "";
    for (let k = permIdx + 1; k < lines.length; k++) {
      const q = lines[k];
      if (q && q.toLowerCase() !== "patterns") {
        question = q;
        break;
      }
    }
    if (!question) question = "Permission required";
    return { question, options };
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = ["opencode"];
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    return parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    void skipPermissions;
    const parts = ["opencode", "--session", host.shell.quote(sessionId)];
    return parts.join(" ");
  },
  detectSessionId(_cwd, _exclude, _maxAgeMs) {
    return null;
  },
  checkIntegration() {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const pluginExists = !!pluginPath && host.fs.fileExists(pluginPath);
    const pluginBody = pluginExists ? String(host.fs.readFile(pluginPath) || "") : "";
    const pluginCurrent = pluginExists && pluginBody.indexOf(CODETERM_OPENCODE_PLUGIN_MARKER) !== -1 && pluginBody.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1;
    if (!cfg || !host.fs.fileExists(cfg)) {
      return {
        config_file_exists: false,
        plugin_file_exists: pluginExists,
        plugin_file_current: pluginCurrent,
        plugin_installed: false
      };
    }
    const json = host.fs.readJson(cfg);
    if (!json) {
      return {
        config_file_exists: true,
        plugin_file_exists: pluginExists,
        plugin_file_current: pluginCurrent,
        plugin_installed: false
      };
    }
    const target = opencodePluginEntry();
    const entries = json.plugin;
    const installed = Array.isArray(entries) && entries.some((entry) => {
      return opencodePluginEntryMatches(entry, target);
    });
    return {
      config_file_exists: true,
      plugin_file_exists: pluginExists,
      plugin_file_current: pluginCurrent,
      plugin_installed: installed
    };
  },
  installIntegration(apiPort) {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const target = opencodePluginEntry();
    if (!cfg || !pluginPath || !target) return { ok: false, error: "could not resolve opencode config path" };
    const json = host.fs.fileExists(cfg) ? host.fs.readJson(cfg) || { "$schema": "https://opencode.ai/config.json" } : { "$schema": "https://opencode.ai/config.json" };
    if (!json || typeof json !== "object" || Array.isArray(json)) {
      return { ok: false, error: "opencode.json root must be an object" };
    }
    if (!Array.isArray(json.plugin)) json.plugin = [];
    json.plugin = json.plugin.filter((entry) => !opencodePluginEntryMatches(entry, target));
    json.plugin.push(target);
    if (!host.fs.writeFile(pluginPath, opencodeBindingPluginSource(apiPort | 0))) {
      return { ok: false, error: "writeFile failed for " + pluginPath };
    }
    if (!host.fs.writeJson(cfg, json)) {
      return { ok: false, error: "writeJson failed for " + cfg };
    }
    host.log("info", "[opencode-plugin] session binding plugin installed at " + pluginPath);
    return { ok: true };
  },
  uninstallIntegration() {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const target = opencodePluginEntry();
    if (cfg && host.fs.fileExists(cfg)) {
      const json = host.fs.readJson(cfg);
      if (json && typeof json === "object" && Array.isArray(json.plugin)) {
        json.plugin = json.plugin.filter((entry) => !opencodePluginEntryMatches(entry, target));
        if (!host.fs.writeJson(cfg, json)) {
          return { ok: false, error: "writeJson failed for " + cfg };
        }
      }
    }
    if (pluginPath && host.fs.fileExists(pluginPath) && !host.fs.removeFile(pluginPath)) {
      return { ok: false, error: "removeFile failed for " + pluginPath };
    }
    return { ok: true };
  },
  ensureIntegrationOnStartup(apiPort) {
    const status = this.checkIntegration();
    if (status && status.plugin_installed && status.plugin_file_exists && status.plugin_file_current) return { ok: true };
    return this.installIntegration(apiPort);
  },
  sessionExists(_cwd, sessionId) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return false;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT 1 AS one FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    return rows.length > 0;
  },
  // Session creation time (unix ms). Lets the host apply its launch-window
  // birth-time guard to sqlite-backed sessions, which have no per-session file.
  sessionCreatedMs(_cwd, sessionId) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT time_created FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    if (!rows.length) return null;
    const tc = rows[0].time_created;
    return typeof tc === "number" ? tc : null;
  },
  detectActiveSession(_cwd, _currentSid) {
    return null;
  },
  detectSessionModel(cwd, sessionId) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    if (sessionId) {
      const model = scanMessagesForModel(
        db,
        "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 20",
        [sessionId]
      );
      if (model) return model;
    }
    return scanMessagesForModel(
      db,
      "SELECT m.data AS data FROM message m JOIN session s ON s.id = m.session_id WHERE (s.directory = ?1 OR s.directory = ?2) ORDER BY m.time_created DESC LIMIT 50",
      [cwd.replace(/\\/g, "/"), cwd.replace(/\//g, "\\")]
    );
  },
  // opencode keeps chat in opencode.db (part JOIN message), not a JSONL the
  // host can tail — so the chat UI reads it structured. cursor is the max
  // part.time_created seen, passed back to fetch only newer parts next poll.
  usesStructuredChat() {
    return true;
  },
  readStructuredChat(_cwd, sessionId, cursor) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return { messages: [], cursor: cursor || null };
    const sinceMs = cursor ? parseInt(cursor, 10) || 0 : 0;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT p.id AS id, p.time_created AS ts, p.data AS part, m.data AS msg FROM part p JOIN message m ON m.id = p.message_id WHERE p.session_id = ?1 AND p.time_created >= ?2 ORDER BY p.time_created ASC, p.id ASC",
      [sessionId, sinceMs]
    );
    const messages = [];
    let maxTs = sinceMs;
    for (let i = 0; i < rows.length; i++) {
      const ts = rows[i].ts || 0;
      if (ts > maxTs) maxTs = ts;
      const part = safeJson(rows[i].part);
      if (!part) continue;
      let role = "assistant";
      const msg = safeJson(rows[i].msg);
      if (msg && msg.role) role = msg.role;
      const cm = partToChatMsg(part, role, String(rows[i].id), ts);
      if (cm) messages.push(cm);
    }
    return { messages, cursor: String(maxTs) };
  }
};
function safeJson(s) {
  try {
    return JSON.parse(s);
  } catch (_) {
    return null;
  }
}
function partToChatMsg(part, role, id, tsMs) {
  const timestamp = String(tsMs);
  const type = part && part.type;
  if (type === "text") {
    const txt = part.text || "";
    if (!txt.trim()) return null;
    return { id, type: role === "user" ? "user" : "assistant", content: txt, timestamp };
  }
  if (type === "reasoning") {
    const rtxt = part.text || "";
    if (!rtxt.trim()) return null;
    return { id, type: "thinking", content: rtxt, timestamp };
  }
  if (type === "tool") {
    const tool = part.tool || "tool";
    const state = part.state || {};
    const out = { id, type: "tool_call", content: "", timestamp, toolName: tool };
    if (state.input !== void 0) out.toolInput = state.input;
    if (typeof state.output === "string") out.toolResult = state.output;
    return out;
  }
  return null;
}
function opencodeDbPath() {
  let base = host.envGet("XDG_DATA_HOME");
  if (!base) base = host.expandHome("~/.local/share");
  if (!base) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return base + sep + "opencode" + sep + "opencode.db";
}
function opencodeConfigDir() {
  const override = host.envGet("OPENCODE_CONFIG_DIR");
  if (override) return override;
  return host.expandHome("~/.config/opencode");
}
function opencodeConfigPath() {
  const dir = opencodeConfigDir();
  if (!dir) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + "opencode.json";
}
function opencodeBindingPluginPath() {
  const dir = opencodeConfigDir();
  if (!dir) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return dir + sep + CODETERM_OPENCODE_PLUGIN_FILE;
}
function opencodePluginEntry() {
  const path = opencodeBindingPluginPath();
  return path ? path.replace(/\\/g, "/") : null;
}
function opencodePluginEntryMatches(entry, target) {
  if (!target) return false;
  let value = null;
  if (typeof entry === "string") value = entry;
  else if (Array.isArray(entry) && typeof entry[0] === "string") value = entry[0];
  if (!value) return false;
  return value.replace(/\\/g, "/") === target;
}
function opencodeBindingPluginSource(apiPort) {
  return [
    "// " + CODETERM_OPENCODE_PLUGIN_MARKER,
    'const PROVIDER_ID = "opencode";',
    'const FALLBACK_PORT = "' + String(apiPort || 7686) + '";',
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
    '  return typeof value === "string" && value.startsWith("ses_") ? value : null;',
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
    '    "content-type": "application/json",',
    '    "x-codeterm-agent-provider": PROVIDER_ID,',
    "  };",
    "  if (process.env.CODETERM_SESSION_BINDING_NONCE) {",
    '    headers["x-codeterm-binding-nonce"] = process.env.CODETERM_SESSION_BINDING_NONCE;',
    "  }",
    "  if (process.env.CODETERM_API_TOKEN) {",
    "    headers.authorization = `Bearer ${process.env.CODETERM_API_TOKEN}`;",
    "  }",
    "  await fetch(url, {",
    '    method: "POST",',
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
    '        event?.type === "session.created" ||',
    '        event?.type === "session.updated" ||',
    '        event?.type === "tui.session.select" ||',
    '        event?.type === "message.updated" ||',
    '        event?.type === "message.part.updated"',
    "      ) {",
    "        await report(eventSessionID(event), event.type, eventDirectory(event, directory));",
    "      }",
    "    },",
    '    async "chat.message"(input) {',
    '      await report(input?.sessionID, "chat.message", directory);',
    "    },",
    '    async "tool.execute.before"(input) {',
    '      await report(input?.sessionID, "tool.execute.before", directory);',
    "    },",
    '    async "shell.env"(input, output) {',
    '      await report(input?.sessionID, "shell.env", input?.cwd || directory);',
    "      return output;",
    "    },",
    "  };",
    "}",
    ""
  ].join("\n");
}
function scanMessagesForModel(db, sql, params) {
  const rows = host.fs.sqliteQuery(db, sql, params);
  for (let i = 0; i < rows.length; i++) {
    const raw = rows[i].data;
    if (!raw) continue;
    try {
      const v = JSON.parse(raw);
      if (v && v.role === "assistant" && typeof v.modelID === "string" && v.modelID) {
        return v.modelID;
      }
    } catch (_) {
    }
  }
  return null;
}
var plugin_default = plugin;
