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

// plugins/provider-opencode/src/helpers.ts
var CODETERM_OPENCODE_PLUGIN_MARKER = "codeterm-opencode-session-binding-v3";
var CODETERM_OPENCODE_PLUGIN_OWNERSHIP_PREFIX = "codeterm-opencode-session-binding-v";
var QUESTION_MENU_RE = /^\s*(?:[>❯▶➤→●]\s*)?(\d+)[.)]\s+(.+?)\s*$/;
var QUESTION_MENU_SELECTORS = [">", "\u276F", "\u25B6", "\u27A4", "\u2192", "\u25CF"];
var PICKER_TITLE_RE = /^(Select model|Connect a provider)\b.*\besc\b/i;
var lastPromptShape = "horizontal";
function parsePermissionPrompt(text) {
  const lines = String(text || "").split("\n").map(cleanPromptLine);
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
  let question = "";
  for (let k = permIdx + 1; k < lines.length; k++) {
    const q = lines[k];
    if (q && q.toLowerCase() !== "patterns") {
      question = q;
      break;
    }
  }
  lastPromptShape = "horizontal";
  return {
    question: question || "Permission required",
    options: labels.map((label, idx) => [label, String(idx + 1)])
  };
}
function collectQuestionMenuPrompt(text) {
  const lines = String(text || "").split("\n").map(cleanPromptLine);
  const start = Math.max(0, lines.length - 30);
  const rows = [];
  for (let i = start; i < lines.length; i++) {
    const line = lines[i];
    const m = QUESTION_MENU_RE.exec(line);
    if (m && m[2].trim()) {
      rows.push({ value: m[1], label: m[2].trim(), idx: i, sel: isQuestionMenuSelected(line) });
    }
  }
  if (rows.length < 2) return null;
  const cluster = [];
  let prev = rows[rows.length - 1].idx;
  for (let i = rows.length - 1; i >= 0; i--) {
    if (prev - rows[i].idx <= 2 || cluster.length === 0) {
      cluster.unshift(rows[i]);
      prev = rows[i].idx;
    } else break;
  }
  if (cluster.length < 2 || !hasQuestionMenuFooter(lines, cluster[cluster.length - 1].idx)) return null;
  let question = "";
  for (let i = cluster[0].idx - 1; i >= start && cluster[0].idx - i <= 6; i--) {
    const q = lines[i];
    if (q && q.charAt(0) !== "#" && !isQuestionMenuFooter(q)) {
      question = q;
      break;
    }
  }
  return { question: question || "Select an option", cluster };
}
function parseQuestionMenuPrompt(text) {
  const c = collectQuestionMenuPrompt(text);
  if (!c) return null;
  lastPromptShape = "vertical";
  return {
    question: c.question,
    options: c.cluster.map((row) => [row.label, row.value])
  };
}
function collectModalPickerPrompt(text) {
  const rawLines = String(text || "").split("\n");
  const titleIdx = findModalPickerTitle(text);
  if (titleIdx < 0) return null;
  const searchIdx = findModalSearchLine(rawLines, titleIdx);
  if (searchIdx < 0) return null;
  const modalCol = rawLines[searchIdx].indexOf("Search");
  if (modalCol < 0) return null;
  const question = cleanPromptLine(rawLines[titleIdx].slice(modalCol)).replace(/\s+esc$/i, "").trim();
  const rows = [];
  let nextValue = 1;
  for (let i = searchIdx + 1; i < Math.min(rawLines.length, searchIdx + 22); i++) {
    const raw = rawLines[i];
    const label = modalPickerLabel(raw, modalCol);
    if (!label) continue;
    if (isModalPickerSection(label)) continue;
    rows.push({
      value: String(nextValue++),
      label,
      idx: i,
      sel: raw.slice(Math.max(0, modalCol - 4), modalCol + 2).indexOf("\u25CF") !== -1
    });
  }
  if (rows.length < 2) return null;
  return { question: question || "Select an option", cluster: rows };
}
function parseModalPickerPrompt(text) {
  const c = collectModalPickerPrompt(text);
  if (!c) return null;
  lastPromptShape = "vertical";
  return {
    question: c.question,
    options: c.cluster.map((row) => [row.label, row.value])
  };
}
function cleanPromptLine(line) {
  return String(line).replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").replace(/[│┃|╎╏┆┊╭╮╯╰╹▀]/g, " ").replace(/^[\s←⚠»·]+/, "").replace(/[\s·»]+$/, "").trim();
}
function chunkHasQuestionMenuRow(chunk) {
  const lines = String(chunk || "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (QUESTION_MENU_RE.test(cleanPromptLine(lines[i]))) return true;
  }
  return false;
}
function questionMenuHighlightIndex(text) {
  const c = collectQuestionMenuPrompt(text);
  if (!c) return -1;
  return c.cluster.findIndex((row) => row.sel);
}
function verticalPromptHighlightIndex(text) {
  const modal = collectModalPickerPrompt(text);
  if (modal) return modal.cluster.findIndex((row) => row.sel);
  return questionMenuHighlightIndex(text);
}
function isVerticalPrompt(text) {
  return !!collectQuestionMenuPrompt(text) || !!collectModalPickerPrompt(text);
}
function isQuestionMenuSelected(line) {
  const first = String(line || "").trim().charAt(0);
  return QUESTION_MENU_SELECTORS.indexOf(first) !== -1;
}
function findModalPickerTitle(text) {
  const lines = String(text || "").split("\n");
  const start = Math.max(0, lines.length - 40);
  for (let i = lines.length - 1; i >= start; i--) {
    if (PICKER_TITLE_RE.test(cleanPromptLine(lines[i]))) return i;
  }
  return -1;
}
function findModalSearchLine(lines, titleIdx) {
  for (let i = titleIdx + 1; i < Math.min(lines.length, titleIdx + 8); i++) {
    if (cleanPromptLine(lines[i]) === "Search") return i;
  }
  return -1;
}
function modalPickerLabel(rawLine, modalCol) {
  const cropped = rawLine.slice(modalCol, modalCol + 54);
  const label = cleanPromptLine(cropped).replace(/\s+/g, " ").trim();
  if (!label) return "";
  if (/^\/.+\b\d+\.\d+\.\d+\b/.test(label)) return "";
  return label;
}
function isModalPickerSection(label) {
  const lc = label.toLowerCase();
  return lc === "search" || lc === "popular" || lc === "providers" || lc === "popular providers" || lc.indexOf("view all providers") === 0;
}
function hasQuestionMenuFooter(lines, lastOptionIdx) {
  for (let i = lastOptionIdx + 1; i < Math.min(lines.length, lastOptionIdx + 7); i++) {
    if (isQuestionMenuFooter(lines[i])) return true;
  }
  return false;
}
function isQuestionMenuFooter(line) {
  const lc = String(line || "").toLowerCase();
  return lc.indexOf("select") !== -1 && lc.indexOf("submit") !== -1 && lc.indexOf("dismiss") !== -1;
}
function discoverOpencodeModels() {
  const raw = host.exec(JSON.stringify({
    bin: "opencode",
    args: ["models"],
    timeoutMs: 3e3
  }));
  const result = safeJson(raw);
  if (!result || result.code !== 0 || typeof result.stdout !== "string") return null;
  const lines = result.stdout.split("\n");
  const models = [];
  for (let i = 0; i < lines.length; i++) {
    const id = lines[i].trim();
    if (!isModelId(id)) continue;
    if (isNonChatModel(id)) continue;
    models.push({ id, displayName: displayNameFromId(id), group: groupFromModelId(id) });
  }
  return models.length ? models : null;
}
function groupFromModelId(id) {
  const slash = id.split("/");
  if (slash.length < 2) return void 0;
  const provider = slash[0].trim();
  if (!provider) return void 0;
  if (provider.toLowerCase() === "opencode") return "OpenCode Zen";
  return provider;
}
function isModelId(value) {
  return /^[a-z0-9][a-z0-9_.-]*\/[a-z0-9][a-z0-9_.-]*$/i.test(value);
}
var NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle", "image", "video", "imagine"];
function isNonChatModel(id) {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  return false;
}
function discoverOpencodeGroupUsage() {
  const db = opencodeDbPath();
  if (!db || !host.fs.fileExists(db)) return [];
  const rows = host.fs.sqliteQuery(
    db,
    "SELECT json_extract(data,'$.providerID') AS g, COUNT(*) AS messages, SUM(COALESCE(json_extract(data,'$.tokens.input'),0)) AS input, SUM(COALESCE(json_extract(data,'$.tokens.output'),0)) AS output, SUM(COALESCE(json_extract(data,'$.tokens.cache.read'),0)) AS cache_read, SUM(COALESCE(json_extract(data,'$.tokens.cache.write'),0)) AS cache_write, SUM(COALESCE(json_extract(data,'$.cost'),0)) AS cost FROM message WHERE json_extract(data,'$.role')='assistant' AND json_extract(data,'$.providerID') IS NOT NULL GROUP BY g",
    []
  );
  const out = [];
  for (let i = 0; i < rows.length; i++) {
    const group = rows[i].g;
    if (typeof group !== "string" || !group) continue;
    const costMicros = Math.round(finiteNumber(rows[i].cost) * 1e6);
    const entry = {
      group,
      messages: finiteNumber(rows[i].messages),
      input_tokens: finiteNumber(rows[i].input),
      output_tokens: finiteNumber(rows[i].output),
      cache_read_tokens: finiteNumber(rows[i].cache_read),
      cache_write_tokens: finiteNumber(rows[i].cache_write),
      cost_micros: costMicros
    };
    if (costMicros > 0) entry.spend_cents = Math.round(costMicros / 1e4);
    const remaining = groupBalanceCents(group);
    if (remaining != null) entry.spend_remaining_cents = remaining;
    out.push(entry);
  }
  return out;
}
var BALANCE_BY_GROUP = {
  deepseek: { url: "https://api.deepseek.com/user/balance", secret: "deepseekKey" }
};
var BALANCE_CREDENTIAL = "opencodeAuth";
var BALANCE_TTL_MS = 6e4;
var BALANCE_TIMEOUT_MS = 4e3;
var balanceCache = null;
function groupBalanceCents(group) {
  if (!BALANCE_BY_GROUP[group]) return null;
  const now = host.unixNowMs();
  if (balanceCache && balanceCache.group === group && now - balanceCache.atMs < BALANCE_TTL_MS) {
    return balanceCache.cents;
  }
  const cents = fetchBalanceCents(group);
  balanceCache = { group, atMs: now, cents };
  return cents;
}
function fetchBalanceCents(group) {
  const spec = BALANCE_BY_GROUP[group];
  if (!spec || typeof host.fetch !== "function") return null;
  let response = null;
  try {
    response = safeJson(host.fetch(JSON.stringify({
      url: spec.url,
      method: "GET",
      timeoutMs: BALANCE_TIMEOUT_MS,
      credential: {
        id: BALANCE_CREDENTIAL,
        headers: { Authorization: "Bearer {" + spec.secret + "}" }
      }
    })));
  } catch (_) {
    return null;
  }
  if (!response || response.status !== 200 || typeof response.body !== "string") return null;
  const body = safeJson(response.body);
  const infos = body && Array.isArray(body.balance_infos) ? body.balance_infos : [];
  let chosen = null;
  for (let i = 0; i < infos.length; i++) {
    if (infos[i] && infos[i].currency === "USD") {
      chosen = infos[i];
      break;
    }
  }
  if (!chosen && infos.length > 0) chosen = infos[0];
  if (!chosen || typeof chosen.total_balance !== "string") return null;
  const total = parseFloat(chosen.total_balance);
  return Number.isFinite(total) ? Math.round(total * 100) : null;
}
function finiteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}
function safeJson(s) {
  try {
    return JSON.parse(String(s || "null"));
  } catch (_) {
    return null;
  }
}
function normalizeSessionDirectory(value) {
  let normalized = String(value || "");
  if (host.platform() === "windows") {
    normalized = normalized.replace(/\\/g, "/");
    while (normalized.length > 1 && normalized.endsWith("/") && !/^[a-z]:\/$/i.test(normalized)) {
      normalized = normalized.slice(0, -1);
    }
    return normalized.toLowerCase();
  }
  while (normalized.length > 1 && normalized.endsWith("/")) normalized = normalized.slice(0, -1);
  return normalized;
}
function parseStructuredCursor(cursor) {
  if (cursor == null) return [0, []];
  const parsed = safeJson(cursor || "");
  if (!Array.isArray(parsed) || parsed.length !== 2) return null;
  if (typeof parsed[0] !== "number" || !Number.isSafeInteger(parsed[0]) || parsed[0] < 0) return null;
  if (!Array.isArray(parsed[1]) || !parsed[1].every((id) => typeof id === "string" && id.length > 0)) return null;
  const ts = parsed[0];
  const ids = parsed[1];
  return [ts, ids];
}
function displayNameFromId(id) {
  const slash = id.split("/");
  const provider = slash.length > 1 ? slash[0] : "";
  const leaf = slash[slash.length - 1] || id;
  const words = prettyLeaf(leaf);
  if (provider.toLowerCase() === "opencode") return "OpenCode Zen " + words;
  return words;
}
function prettyLeaf(leaf) {
  const parts = leaf.split("-").filter(Boolean);
  const out = [];
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (/^\d+$/.test(p) && out.length && /\d$/.test(out[out.length - 1])) {
      out[out.length - 1] = out[out.length - 1] + "." + p;
    } else {
      out.push(capitalizeToken(p));
    }
  }
  return out.join(" ");
}
function capitalizeToken(token) {
  if (!token) return token;
  if (token === "gpt") return "GPT";
  if (token === "ai") return "AI";
  return token.charAt(0).toUpperCase() + token.substring(1);
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
    const canonical = canonicalToolFields(tool, state.input);
    const out = {
      id,
      type: "tool_call",
      content: "",
      timestamp,
      toolName: tool,
      toolKind: canonical.kind,
      toolSummary: canonical.summary,
      toolDetail: canonical.detail,
      toolEdits: canonical.edits
    };
    if (state.input !== void 0) out.toolInput = state.input;
    if (typeof state.output === "string") out.toolResult = state.output;
    return out;
  }
  return null;
}
function canonicalToolFields(name, input) {
  const tool = String(name || "").trim();
  const value = input && typeof input === "object" && !Array.isArray(input) ? input : {};
  const text = (key) => typeof value[key] === "string" ? value[key] : "";
  const command = text("command") || text("cmd");
  const path = text("file_path") || text("filePath") || text("path");
  const pattern = text("pattern");
  const edits = extractCanonicalEdits(value);
  const lower = tool.toLowerCase();
  if (lower === "bash" || lower === "shell" || lower === "exec" || lower === "run") {
    return { kind: "command", summary: cleanToolText(command, "Command"), detail: command, edits: [] };
  }
  if (lower === "edit" || lower === "multiedit" || lower === "write") {
    return { kind: "edit", summary: path || "File edit", detail: path, edits };
  }
  if (lower === "read") {
    return { kind: "read", summary: path || "Read file", detail: path, edits: [] };
  }
  if (lower === "glob" || lower === "grep" || lower === "search") {
    return { kind: "search", summary: cleanToolText(pattern || path, "Search"), detail: pattern || path, edits: [] };
  }
  return { kind: "generic", summary: cleanToolText(tool, "Tool"), detail: tool, edits };
}
function extractCanonicalEdits(input) {
  const parentPath = typeof input.file_path === "string" ? input.file_path : typeof input.filePath === "string" ? input.filePath : typeof input.path === "string" ? input.path : "";
  const pair = (value, fallbackPath) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const item = value;
    const oldValue = typeof item.old_string === "string" ? item.old_string : item.oldString;
    const newValue = typeof item.new_string === "string" ? item.new_string : item.newString;
    if (typeof oldValue !== "string" && typeof newValue !== "string") return null;
    const path = typeof item.file_path === "string" ? item.file_path : typeof item.filePath === "string" ? item.filePath : typeof item.path === "string" ? item.path : fallbackPath;
    return { path, old: typeof oldValue === "string" ? oldValue : "", new: typeof newValue === "string" ? newValue : "" };
  };
  if (Array.isArray(input.edits)) return input.edits.map((edit) => pair(edit, parentPath)).filter((e) => e !== null);
  const single = pair(input, parentPath);
  return single ? [single] : [];
}
function cleanToolText(value, fallback) {
  const text = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  if (!text) return fallback;
  return text.length > 120 ? text.slice(0, 117) + "..." : text;
}
function opencodeDbPath() {
  const windows = host.platform() === "windows";
  const sep = windows ? "\\" : "/";
  const configuredDb = host.envGet("OPENCODE_DB");
  if (configuredDb === ":memory:") return null;
  if (configuredDb) {
    const absolute = windows ? /^[A-Za-z]:[\\/]|^[\\/]{1,2}/.test(configuredDb) : configuredDb.startsWith("/");
    if (absolute) return configuredDb;
  }
  const base = host.envGet("XDG_DATA_HOME") || host.expandHome("~/.local/share");
  if (!base) return null;
  const dataDir = base + sep + "opencode";
  return configuredDb ? dataDir + sep + configuredDb : dataDir + sep + "opencode.db";
}
function opencodeSessionRecord(row, db) {
  return {
    sessionId: row.id,
    provider: "opencode",
    projectPath: typeof row.directory === "string" && row.directory ? row.directory : null,
    title: typeof row.title === "string" && row.title ? row.title : null,
    summary: null,
    filePath: db,
    createdTs: typeof row.time_created === "number" ? Math.floor(row.time_created / 1e3) : null,
    modifiedTs: typeof row.time_updated === "number" ? Math.floor(row.time_updated / 1e3) : null
  };
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
var CODETERM_OPENCODE_PLUGIN_FILE = "codeterm-session-binding.js";
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
function isCurrentOpencodeBindingPluginSource(text) {
  const currentHeader = "// " + CODETERM_OPENCODE_PLUGIN_MARKER;
  const hasCurrentHeader = text.startsWith(currentHeader + "\n") || text.startsWith(currentHeader + "\r\n");
  const required = [
    "CODETERM_API_URL",
    "CODETERM_API_TOKEN",
    "CODETERM_SESSION_BINDING_NONCE",
    "CODETERM_TAB_ID",
    "/api/tabs/",
    "x-codeterm-binding-nonce",
    "authorization",
    "sessionID === lastReported"
  ];
  return hasCurrentHeader && required.every((value) => text.includes(value)) && !/CODETERM_API_PORT|FALLBACK_PORT|127\.0\.0\.1|localhost|:\d{2,5}/i.test(text);
}
function ownsOpencodeBindingPluginSource(text) {
  const header = "// " + CODETERM_OPENCODE_PLUGIN_OWNERSHIP_PREFIX;
  return text.startsWith(header) && /^\d+(?:\r?\n|$)/.test(text.slice(header.length));
}
function opencodeBindingPluginSource() {
  return [
    "// " + CODETERM_OPENCODE_PLUGIN_MARKER,
    'const PROVIDER_ID = "opencode";',
    "let lastReported = null;",
    "",
    "function endpoint() {",
    "  const tab = process.env.CODETERM_TAB_ID;",
    "  const token = process.env.CODETERM_API_TOKEN;",
    "  const nonce = process.env.CODETERM_SESSION_BINDING_NONCE;",
    "  if (!tab) return null;",
    "  if (!token || !nonce || !process.env.CODETERM_API_URL) return null;",
    '  const base = process.env.CODETERM_API_URL.replace(/\\/+$/, "");',
    "  return `${base}/api/tabs/${encodeURIComponent(tab)}/hook/session_start`;",
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
    "  if (!sessionID || sessionID === lastReported) return;",
    "  const url = endpoint();",
    "  if (!url) return;",
    "  lastReported = sessionID;",
    "  const headers = {",
    '    "content-type": "application/json",',
    '    "x-codeterm-agent-provider": PROVIDER_ID,',
    '    "x-codeterm-binding-nonce": process.env.CODETERM_SESSION_BINDING_NONCE,',
    "    authorization: `Bearer ${process.env.CODETERM_API_TOKEN}`,",
    "  };",
    "  const delivered = await fetch(url, {",
    '    method: "POST",',
    "    headers,",
    "    body: JSON.stringify({",
    "      provider_id: PROVIDER_ID,",
    "      session_id: sessionID,",
    "      source,",
    "      cwd: directory || process.cwd(),",
    "    }),",
    "  }).then((response) => response.ok, () => false);",
    "  if (!delivered && lastReported === sessionID) lastReported = null;",
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
function scanMessagesForQualifiedModel(db, sessionId) {
  try {
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 20",
      [sessionId]
    );
    for (let i = 0; i < rows.length; i++) {
      const raw = rows[i].data;
      if (!raw) continue;
      try {
        const value = JSON.parse(raw);
        if (value.role === "assistant" && typeof value.providerID === "string" && value.providerID && typeof value.modelID === "string" && value.modelID) {
          return `${value.providerID}/${value.modelID}`;
        }
      } catch (_) {
      }
    }
  } catch (_) {
  }
  return null;
}
function scanSessionDirectory(db, sessionId) {
  try {
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT id, directory FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    const session = rows.find((row) => row.id === sessionId);
    return typeof session?.directory === "string" && session.directory.trim() ? session.directory : null;
  } catch (_) {
    return null;
  }
}

// plugins/provider-opencode/src/plugin.ts
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
var ASSIGNED_TASK_START_PROMPT = "Execute the assigned task. Follow the configured instructions.";
var IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";
function existingBindingPluginIsUnowned(pluginPath) {
  if (!host.fs.fileExists(pluginPath)) return false;
  return !ownsOpencodeBindingPluginSource(String(host.fs.readFile(pluginPath) || ""));
}
function starterPrompt(params) {
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return void 0;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}
function withPadPermissionConfig(command, instructions, cleanup) {
  const platform = host.platform();
  const config = {
    permission: { external_directory: { "*": "ask" } },
    ...instructions ? { instructions } : {}
  };
  if (platform === "windows") {
    const encoded = host.shell.quoteFor(JSON.stringify(config), platform);
    return `$ct_config = ${encoded} | ConvertFrom-Json; $ct_pad_native = ([string]$env:CODETERM_PAD_DIR).TrimEnd([char[]]@([char]92, [char]47)); $ct_pad = $ct_pad_native -replace '\\\\', '/'; if ($ct_pad_native) { $ct_config.permission.external_directory | Add-Member -NotePropertyName ($ct_pad_native + '\\*') -NotePropertyValue 'allow' -Force; $ct_config.permission.external_directory | Add-Member -NotePropertyName ($ct_pad + '/*') -NotePropertyValue 'allow' -Force }; $env:OPENCODE_CONFIG_CONTENT = $ct_config | ConvertTo-Json -Depth 10 -Compress; try { ${command} } finally { Remove-Item Env:OPENCODE_CONFIG_CONTENT -ErrorAction SilentlyContinue; ${cleanup || ""} }`;
  }
  const permissionJson = JSON.stringify({ permission: config.permission });
  const prefix = permissionJson.slice(0, -3);
  const suffix = `}}${instructions ? `,"instructions":${JSON.stringify(instructions)}` : ""}}`;
  const q = (value) => host.shell.quoteFor(value, platform);
  const finish = cleanup ? `; ct_status=$?; unset OPENCODE_CONFIG_CONTENT; ${cleanup}; exit $ct_status` : `; unset OPENCODE_CONFIG_CONTENT`;
  return `ct_pad_dir=$(printf '%s' "${"${CODETERM_PAD_DIR:-}"}" | tr '\\\\' '/' | sed 's/"/\\\\\\"/g'); OPENCODE_CONFIG_CONTENT=${q(prefix)}; if [ -n "$ct_pad_dir" ]; then OPENCODE_CONFIG_CONTENT="$OPENCODE_CONFIG_CONTENT,\\"$ct_pad_dir/*\\":\\"allow\\""; fi; OPENCODE_CONFIG_CONTENT="$OPENCODE_CONFIG_CONTENT"$(printf '%s' ${q(suffix)}); export OPENCODE_CONFIG_CONTENT; ${command}${finish}`;
}
var plugin = {
  starterPromptText(prompt) {
    return prompt === "team_bootstrap" ? void 0 : starterPrompt({ starterPrompt: prompt });
  },
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
    const lc = c.toLowerCase();
    return c.indexOf("Permission required") !== -1 || lc.indexOf("approve") !== -1 || chunkHasQuestionMenuRow(c) || findModalPickerTitle(c) >= 0 || lc.indexOf("select") !== -1 && lc.indexOf("submit") !== -1 && lc.indexOf("dismiss") !== -1;
  },
  detectEvents(text) {
    const t = String(text || "");
    const events = [];
    if (t.indexOf("Permission required") !== -1 || t.indexOf("Allow this tool call") !== -1 || t.indexOf("Approve this command") !== -1 || parseQuestionMenuPrompt(t) || parseModalPickerPrompt(t)) {
      events.push("permission_request");
    }
    return events;
  },
  // off hot reads. `null` when the CLI cannot answer: the host keeps its seed.
  discoverModels() {
    return discoverOpencodeModels();
  },
  // Cumulative usage per upstream company comes from OpenCode's session store.
  // Model-id prefixes match picker groups, keeping vendor knowledge out of core.
  modelGroupUsage() {
    return discoverOpencodeGroupUsage();
  },
  // because response bytes depend on horizontal vs vertical navigation.
  parsePrompt(text) {
    const screen = String(text || "");
    return parseQuestionMenuPrompt(screen) || parseModalPickerPrompt(screen) || parsePermissionPrompt(screen);
  },
  // opencode has both horizontal permission rows and vertical question menus.
  buildPromptResponse(optionIndex, screen) {
    const DOWN = "G1tC";
    const UP = "G1tB";
    const RIGHT = "G1tD";
    const ENTER = "DQ==";
    const steps = [];
    const target = optionIndex > 0 ? optionIndex : 0;
    const promptShape = screen && isVerticalPrompt(String(screen)) ? "vertical" : lastPromptShape;
    if (promptShape === "vertical") {
      const current = screen ? verticalPromptHighlightIndex(String(screen)) : -1;
      const start = current >= 0 ? current : 0;
      const delta = target - start;
      const move = delta >= 0 ? DOWN : UP;
      for (let i = 0; i < Math.abs(delta); i++) steps.push(move);
    } else {
      for (let i = 0; i < target; i++) steps.push(RIGHT);
    }
    steps.push(ENTER);
    return steps;
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = [];
    let localTaskPath = null;
    if (p.task) {
      const taskPath = host.fs.writeTempFile(p.task, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write OpenCode task instructions");
      const platform = host.platform();
      const taskName = taskPath.split(/[\\/]/).pop().replace(/[^A-Za-z0-9._-]/g, "-");
      localTaskPath = `.codeterm/${taskName}`;
      const source = host.shell.quoteFor(taskPath, platform);
      const local = host.shell.quoteFor(localTaskPath, platform);
      parts.push(platform === "windows" ? `New-Item -ItemType Directory -Force -Path '.codeterm' | Out-Null; Copy-Item -LiteralPath ${source} -Destination ${local} -Force;` : `mkdir -p .codeterm && cp -- ${source} ${local} &&`);
    }
    parts.push("opencode");
    if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
    const sps = p.structuredPromptServer;
    if (sps && sps.port) {
      const platform = host.platform();
      parts.push("--hostname", host.shell.quoteFor(String(sps.hostname || "127.0.0.1"), platform));
      parts.push("--port", host.shell.quoteFor(String(sps.port), platform));
    }
    if (p.sessionId) {
      parts.push("--session", host.shell.quoteFor(String(p.sessionId), host.platform()));
    }
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    const prompt = starterPrompt(p);
    if (p.task && prompt) {
      parts.push("--prompt", host.shell.quoteFor(prompt, host.platform()));
    }
    const cleanup = localTaskPath ? `${host.platform() === "windows" ? "Remove-Item -LiteralPath " : "rm -f -- "}${host.shell.quoteFor(localTaskPath, host.platform())}${host.platform() === "windows" ? " -Force -ErrorAction SilentlyContinue;" : ""}` : "";
    return withPadPermissionConfig(parts.join(" "), localTaskPath ? [localTaskPath] : void 0, cleanup);
  },
  buildResumeCommand(sessionId, skipPermissions) {
    return withPadPermissionConfig(resumeLaunch(sessionId, skipPermissions).parts.join(" "));
  },
  // A resumed session re-reads `instructions`, so the composed prompt is
  // re-applied through the same session-config channel buildLaunchCommand uses.
  buildResumeCommandWithContext(params) {
    const p = params || {};
    const platform = host.platform();
    const launch = resumeLaunch(p.sessionId, p.skipPermissions);
    const parts = launch.parts;
    const args = p.args ? resumeLaunchArgs(p.args, p.sessionId) : [];
    for (let i = 0; i < args.length; i++) {
      const arg = String(args[i]);
      if (arg === "-m" || arg === "--model") {
        parts.push(arg, host.shell.quoteFor(String(args[i + 1] || ""), platform));
        i++;
      } else {
        parts.push(arg.startsWith("-") ? arg : host.shell.quoteFor(arg, platform));
      }
    }
    const systemPrompt = p.systemPrompt;
    if (!systemPrompt || !systemPrompt.trim()) return withPadPermissionConfig(parts.join(" "));
    const taskPath = host.fs.writeTempFile(systemPrompt, ".ct-task.md");
    if (!taskPath) return withPadPermissionConfig(parts.join(" "));
    const taskName = taskPath.split(/[\\/]/).pop().replace(/[^A-Za-z0-9._-]/g, "-");
    const localTaskPath = `.codeterm/${taskName}`;
    const source = host.shell.quoteFor(taskPath, platform);
    const local = host.shell.quoteFor(localTaskPath, platform);
    const project = launch.directory ? host.shell.quoteFor(launch.directory, platform) : null;
    const prologue = platform === "windows" ? `${project ? `Set-Location -LiteralPath ${project} -ErrorAction Stop; ` : ""}New-Item -ItemType Directory -Force -Path '.codeterm' | Out-Null; Copy-Item -LiteralPath ${source} -Destination ${local} -Force;` : `${project ? `cd -- ${project} && ` : ""}mkdir -p .codeterm && cp -- ${source} ${local} &&`;
    const cleanup = platform === "windows" ? `Remove-Item -LiteralPath ${local} -Force -ErrorAction SilentlyContinue;` : `rm -f -- ${local}`;
    return withPadPermissionConfig(`${prologue} ${parts.join(" ")}`, [localTaskPath], cleanup);
  },
  // absent from opencode's env, or event field structure changed in a new release).
  // agent_session_id.is_some() short-circuits the Rust call-site before this runs.
  detectSessionId(cwd, exclude, maxAgeMs) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const targetDirectory = normalizeSessionDirectory(cwd);
    if (!targetDirectory) return null;
    const windowMs = maxAgeMs != null && maxAgeMs > 0 ? maxAgeMs : 3e5;
    const excludeList = Array.isArray(exclude) ? exclude.filter((id) => typeof id === "string") : typeof exclude === "string" && exclude ? [exclude] : [];
    const directoryAliases = targetDirectory.endsWith("/") ? [targetDirectory, targetDirectory] : [targetDirectory, targetDirectory + "/"];
    const directoryExpression = host.platform() === "windows" ? "lower(replace(directory, char(92), '/'))" : "directory";
    const excludeSql = excludeList.length ? " AND id NOT IN (" + excludeList.map((_, index) => "?" + String(index + 4)).join(", ") + ")" : "";
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT id, directory FROM session WHERE time_created > (strftime('%s','now') * 1000 - ?1) AND " + directoryExpression + " IN (?2, ?3)" + excludeSql + " ORDER BY time_created DESC LIMIT 20",
      [windowMs, ...directoryAliases, ...excludeList]
    );
    for (let i = 0; i < rows.length; i++) {
      const id = rows[i].id;
      if (typeof id !== "string" || !id.startsWith("ses_")) continue;
      if (normalizeSessionDirectory(rows[i].directory) !== targetDirectory) continue;
      if (excludeList.indexOf(id) !== -1) continue;
      return id;
    }
    return null;
  },
  enumerateSessions() {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return [];
    const rows = host.fs.sqliteQuery(db, "SELECT id, directory, title, time_created, time_updated FROM session ORDER BY time_updated DESC", []);
    return rows.filter((row) => typeof row.id === "string" && row.id.startsWith("ses_")).map((row) => opencodeSessionRecord(row, db));
  },
  findSession(sessionId) {
    if (typeof sessionId !== "string" || !sessionId.startsWith("ses_")) return null;
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    const rows = host.fs.sqliteQuery(db, "SELECT id, directory, title, time_created, time_updated FROM session WHERE id = ?1 LIMIT 1", [sessionId]);
    return rows.length > 0 && rows[0].id === sessionId ? opencodeSessionRecord(rows[0], db) : null;
  },
  checkIntegration() {
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const pluginExists = !!pluginPath && host.fs.fileExists(pluginPath);
    const pluginBody = pluginExists ? String(host.fs.readFile(pluginPath) || "") : "";
    const pluginOwned = pluginExists && ownsOpencodeBindingPluginSource(pluginBody);
    const pluginCurrent = pluginExists && isCurrentOpencodeBindingPluginSource(pluginBody);
    if (!cfg || !host.fs.fileExists(cfg)) {
      return {
        config_file_exists: false,
        plugin_file_exists: pluginExists,
        plugin_file_owned: pluginOwned,
        plugin_file_current: pluginCurrent,
        plugin_installed: false
      };
    }
    const json = host.fs.readJson(cfg);
    if (!json) {
      return {
        config_file_exists: true,
        plugin_file_exists: pluginExists,
        plugin_file_owned: pluginOwned,
        plugin_file_current: pluginCurrent,
        plugin_installed: false,
        integration_state: "unreadable"
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
      plugin_file_owned: pluginOwned,
      plugin_file_current: pluginCurrent,
      plugin_installed: installed
    };
  },
  installIntegration(_apiPort, options) {
    if (!options || options.consent !== true) {
      return { ok: false, error: "provider integration installation requires explicit local-user consent" };
    }
    const cfg = opencodeConfigPath();
    const pluginPath = opencodeBindingPluginPath();
    const target = opencodePluginEntry();
    if (!cfg || !pluginPath || !target) return { ok: false, error: "could not resolve opencode config path" };
    if (existingBindingPluginIsUnowned(pluginPath)) {
      return { ok: false, error: "refusing to overwrite an unmarked OpenCode plugin file" };
    }
    const json = host.fs.fileExists(cfg) ? host.fs.readJson(cfg) || { "$schema": "https://opencode.ai/config.json" } : { "$schema": "https://opencode.ai/config.json" };
    if (!json || typeof json !== "object" || Array.isArray(json)) {
      return { ok: false, error: "opencode.json root must be an object" };
    }
    if (!Array.isArray(json.plugin)) json.plugin = [];
    json.plugin = json.plugin.filter((entry) => !opencodePluginEntryMatches(entry, target));
    json.plugin.push(target);
    if (!host.fs.writeFile(pluginPath, opencodeBindingPluginSource())) {
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
    if (pluginPath && existingBindingPluginIsUnowned(pluginPath)) {
      return { ok: false, error: "refusing to remove an unmarked OpenCode plugin file" };
    }
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
  ensureIntegrationOnStartup(_apiPort) {
    return this.checkIntegration();
  },
  sessionExists(_cwd, sessionId) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return false;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT id FROM session WHERE id = ?1 LIMIT 1",
      [sessionId]
    );
    return rows.some((row) => row.id === sessionId);
  },
  // opencode keeps every session in one sqlite log, so the source carries the
  // session id: the reader must not charge a sibling's tokens to this tab.
  usageSources(_cwd, sessionId) {
    const db = opencodeDbPath();
    if (!db || !sessionId || !host.fs.fileExists(db)) return [];
    return [{ path: db, sessionId }];
  },
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
  // the model recorded against this session_id. No "latest model in cwd"
  // fallback — that would hand the tab a SIBLING agent's model. Unknown → null.
  detectSessionModel(_cwd, sessionId) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return null;
    if (!sessionId) return null;
    return scanMessagesForModel(
      db,
      "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 20",
      [sessionId]
    );
  },
  usesStructuredChat() {
    return true;
  },
  readStructuredChat(_cwd, sessionId, cursor) {
    const db = opencodeDbPath();
    if (!db || !host.fs.fileExists(db)) return { messages: [], cursor: cursor || null };
    const parsedCursor = parseStructuredCursor(cursor);
    if (!parsedCursor) return { messages: [], cursor: cursor ?? null };
    const [sinceMs, seenAtSinceMs] = parsedCursor;
    const rows = host.fs.sqliteQuery(
      db,
      "SELECT p.id AS id, p.time_created AS ts_created, p.time_updated AS ts_updated, p.data AS part, m.data AS msg FROM part p JOIN message m ON m.id = p.message_id WHERE p.session_id = ?1 AND p.time_updated >= ?2 ORDER BY p.time_updated ASC, p.id ASC",
      [sessionId, sinceMs]
    );
    const messages = [];
    let lastTs = sinceMs;
    let seenAtLastTs = seenAtSinceMs.slice();
    for (let i = 0; i < rows.length; i++) {
      const updated = rows[i].ts_updated || 0;
      const created = rows[i].ts_created || updated;
      const id = String(rows[i].id);
      if (updated > lastTs) {
        lastTs = updated;
        seenAtLastTs = [];
      }
      if (seenAtLastTs.indexOf(id) !== -1) continue;
      seenAtLastTs.push(id);
      const part = safeJson(rows[i].part);
      if (!part) continue;
      let role = "assistant";
      const msg = safeJson(rows[i].msg);
      if (msg && msg.role) role = msg.role;
      const cm = partToChatMsg(part, role, id, created);
      if (cm) messages.push(cm);
    }
    seenAtLastTs.sort();
    return { messages, cursor: JSON.stringify([lastTs, seenAtLastTs]) };
  }
};
function resumeLaunchArgs(args, sessionId) {
  const db = opencodeDbPath();
  const model = db && host.fs.fileExists(db) ? scanMessagesForQualifiedModel(db, sessionId) : null;
  if (!model) return args.map(String);
  const retained = [];
  for (let i = 0; i < args.length; i++) {
    const arg = String(args[i]);
    if (arg === "-m" || arg === "--model") {
      i++;
      continue;
    }
    if (arg.startsWith("-m=") || arg.startsWith("--model=")) continue;
    retained.push(arg);
  }
  retained.push("-m", model);
  return retained;
}
function resumeLaunch(sessionId, skipPermissions) {
  const platform = host.platform();
  const db = opencodeDbPath();
  const directory = db && host.fs.fileExists(db) ? scanSessionDirectory(db, sessionId) : null;
  const parts = ["opencode"];
  if (directory) parts.push(host.shell.quoteFor(directory, platform));
  parts.push("--session", host.shell.quoteFor(sessionId, platform));
  if (skipPermissions) parts.push("--dangerously-skip-permissions");
  return { parts, directory };
}
var plugin_default = plugin;
