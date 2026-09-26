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

// plugins/provider-codex/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  codexProfileInstanceId: () => codexProfileInstanceId,
  codexProfileTabId: () => codexProfileTabId,
  default: () => plugin_default,
  inspectCodexProfiles: () => inspectCodexProfiles,
  manageCodexProfiles: () => manageCodexProfiles,
  releaseCodexProfile: () => releaseCodexProfile,
  sweepCodexProfiles: () => sweepCodexProfiles
});
module.exports = __toCommonJS(plugin_exports);

// plugins/provider-codex/src/models.ts
function discoverCodexModels() {
  return discoverCodexModelsFromCache();
}
function codexModelsCachePath() {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "models_cache.json";
}
function discoverCodexModelsFromCache() {
  const path = codexModelsCachePath();
  if (!path || !host.fs.fileExists(path)) return null;
  const cache = host.fs.readJson(path);
  const raw = cache && cache.models;
  if (!Array.isArray(raw)) return null;
  const kept = [];
  for (let i = 0; i < raw.length; i++) {
    const item = raw[i];
    const id = typeof item.slug === "string" ? item.slug.trim() : "";
    if (!id) continue;
    if (item.visibility === "hide") continue;
    if (isNonChatModel(id)) continue;
    kept.push(item);
  }
  kept.sort((a, b) => priorityOf(a) - priorityOf(b));
  const models = [];
  for (let i = 0; i < kept.length; i++) {
    const item = kept[i];
    const id = item.slug.trim();
    models.push({
      id,
      displayName: typeof item.display_name === "string" && item.display_name.trim() ? item.display_name.trim() : displayNameFromId(id),
      description: typeof item.description === "string" && item.description.trim() ? item.description.trim() : void 0,
      reasoningEfforts: reasoningEffortsFrom(item.supported_reasoning_levels),
      defaultReasoningEffort: defaultEffortFrom(item)
    });
  }
  return models.length ? models : null;
}
function defaultEffortFrom(model) {
  const effort = typeof model.default_reasoning_level === "string" ? model.default_reasoning_level.trim() : "";
  return effort || void 0;
}
function reasoningEffortsFrom(levels) {
  if (!Array.isArray(levels)) return void 0;
  const efforts = [];
  const seen = /* @__PURE__ */ new Set();
  for (const level of levels) {
    const id = typeof level.effort === "string" ? level.effort.trim() : "";
    if (!id || seen.has(id)) continue;
    seen.add(id);
    efforts.push({
      id,
      displayName: id,
      description: typeof level.description === "string" && level.description.trim() ? level.description.trim() : void 0
    });
  }
  return efforts.length ? efforts : void 0;
}
function priorityOf(m) {
  return typeof m.priority === "number" ? m.priority : 1e9;
}
var NON_CHAT_TOKENS = ["embed", "embedding", "embeddings", "tts", "whisper", "moderation", "rerank", "dall", "dalle"];
function isNonChatModel(id) {
  const leaf = (id.split("/").pop() || id).toLowerCase();
  const toks = leaf.split(/[-_.]/);
  for (let i = 0; i < toks.length; i++) {
    if (NON_CHAT_TOKENS.indexOf(toks[i]) !== -1) return true;
  }
  return false;
}
function displayNameFromId(id) {
  const parts = id.split("/");
  const leaf = parts[parts.length - 1] || id;
  return leaf.split("-").filter(Boolean).map(capitalizeToken).join(" ");
}
function capitalizeToken(token) {
  return token ? token.charAt(0).toUpperCase() + token.substring(1) : token;
}

// plugins/provider-codex/src/sessions.ts
var sessionStoreOverrides = {
  root: "CODETERM_CODEX_SESSIONS_ROOT",
  index: "CODETERM_CODEX_SESSION_INDEX_PATH",
  state: "CODETERM_CODEX_STATE_DB_PATH"
};
var STATE_DB_NAME = /^state_(\d+)\.sqlite$/;
function foldSessionIndex(contents) {
  const folded = /* @__PURE__ */ new Map();
  const lines = String(contents || "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    let entry;
    try {
      entry = JSON.parse(line);
    } catch (_) {
      continue;
    }
    if (!entry || typeof entry.id !== "string" || !entry.id.trim()) continue;
    if (typeof entry.thread_name !== "string" || typeof entry.updated_at !== "string") continue;
    const current = folded.get(entry.id);
    if (!current || sessionTimestamp(entry.updated_at) > sessionTimestamp(current.updated_at)) {
      folded.set(entry.id, {
        id: entry.id,
        thread_name: entry.thread_name,
        updated_at: entry.updated_at
      });
    }
  }
  return Array.from(folded.values());
}
function sessionFilePath(_cwd, sessionId) {
  const path = trimmedField(threadRow(sessionId), "rollout_path");
  return path && host.fs.fileExists(path) ? path : null;
}
function sessionExists(_cwd, sessionId) {
  if (!sessionId) return false;
  if (threadRow(sessionId) !== null) return true;
  const store = codexSessionStore();
  return store !== null && sessionIndexEntry(store.indexPath, sessionId) !== null;
}
function detectSessionId(cwd, exclude, maxAgeMs) {
  if (!cwd || !host.fs.fileExists(cwd)) return null;
  const store = codexSessionStore();
  if (!store) return null;
  const entries = readSessionIndex(store.indexPath);
  if (entries.length === 0) return null;
  const excluded = new Set(Array.isArray(exclude) ? exclude : exclude ? [exclude] : []);
  const now = host.unixNowMs ? host.unixNowMs() : Date.now();
  const ageLimit = Number.isFinite(maxAgeMs) && maxAgeMs > 0 ? maxAgeMs : 3e5;
  const matching = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (excluded.has(entry.id)) continue;
    const updatedMs = sessionTimestamp(entry.updated_at);
    if (updatedMs > 0 && now - updatedMs > ageLimit) continue;
    const project = readThreadCwd(entry.id);
    if (project && host.path.equal(project, cwd)) matching.push(entry.id);
  }
  return matching.length === 1 ? matching[0] : null;
}
function enumerateSessions() {
  const store = codexSessionStore();
  if (!store) return [];
  const entries = readSessionIndex(store.indexPath);
  return entries.map((entry) => ({
    sessionId: entry.id,
    provider: "codex",
    projectPath: readThreadCwd(entry.id),
    title: entry.thread_name || null,
    summary: null,
    filePath: store.indexPath,
    createdTs: null,
    modifiedTs: sessionTimestamp(entry.updated_at) > 0 ? Math.floor(sessionTimestamp(entry.updated_at) / 1e3) : null
  }));
}
function findSession(sessionId) {
  const row2 = queryThreadRow(sessionId, "SELECT * FROM threads WHERE id = ? LIMIT 1");
  if (!row2) return null;
  const store = codexSessionStore();
  return {
    sessionId,
    provider: "codex",
    projectPath: resolveThreadCwd(sessionId, row2),
    title: trimmedField(row2, "name") || trimmedField(row2, "title"),
    summary: null,
    filePath: store ? store.indexPath : codexStateDbPath() || "",
    createdTs: unixSeconds(row2.created_at),
    modifiedTs: unixSeconds(row2.updated_at)
  };
}
function detectSessionModel(_cwd, sessionId) {
  return trimmedField(threadRow(sessionId), "model");
}
function detectSessionReasoningEffort(_cwd, sessionId) {
  const path = sessionFilePath(_cwd, sessionId);
  if (!path) return null;
  const contents = host.fs.readFileTail(path, 4 * 1024 * 1024);
  if (typeof contents !== "string") return null;
  const lines = contents.split("\n");
  for (let index = lines.length - 1; index >= 0; index--) {
    let entry;
    try {
      entry = JSON.parse(lines[index]);
    } catch (_) {
      continue;
    }
    if (entry.type !== "turn_context" || typeof entry.payload?.effort !== "string") continue;
    const effort = entry.payload.effort.trim();
    if (effort) return effort;
  }
  return null;
}
function threadRow(sessionId) {
  return queryThreadRow(sessionId, "SELECT model, rollout_path, cwd FROM threads WHERE id = ? LIMIT 1");
}
function queryThreadRow(sessionId, sql) {
  if (!sessionId) return null;
  const db = codexStateDbPath();
  if (!db || !host.fs.fileExists(db)) return null;
  try {
    const rows = host.fs.sqliteQuery(db, sql, [sessionId]);
    return rows.length > 0 ? rows[0] : null;
  } catch (_) {
    return null;
  }
}
function unixSeconds(value) {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.floor(value) : null;
}
function trimmedField(row2, field) {
  const value = row2 ? row2[field] : null;
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
function codexStateDbPath() {
  const override = host.envGet(sessionStoreOverrides.state);
  if (override) return override;
  const sep = host.platform() === "windows" ? "\\" : "/";
  const root = codexSessionsRoot(sep);
  if (!root) return null;
  let entries;
  try {
    entries = host.fs.readDir(root);
  } catch (_) {
    return null;
  }
  let best = null;
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (!entry || !entry.isFile) continue;
    const match = STATE_DB_NAME.exec(entry.name);
    if (!match) continue;
    const version = parseInt(match[1], 10);
    if (!best || version > best.version) best = { name: entry.name, version };
  }
  return best ? root + sep + best.name : null;
}
function extractSessionUuid(stem) {
  if (typeof stem !== "string") return null;
  const prefix = "rollout-";
  if (stem.indexOf(prefix) !== 0) return null;
  const rest = stem.substring(prefix.length);
  if (rest.length <= 20 || rest.charAt(19) !== "-") return null;
  return rest.substring(20);
}
function codexSessionStore() {
  const indexOverride = host.envGet(sessionStoreOverrides.index);
  if (indexOverride) return { indexPath: indexOverride };
  const sep = host.platform() === "windows" ? "\\" : "/";
  const root = codexSessionsRoot(sep);
  if (!root) return null;
  return {
    indexPath: indexOverride || root + sep + "session_index.jsonl"
  };
}
function trimTrailingSeparators(path) {
  let end = path.length;
  while (end > 1 && (path.charAt(end - 1) === "/" || path.charAt(end - 1) === "\\")) end--;
  return path.substring(0, end);
}
function codexSessionsRoot(sep) {
  const override = host.envGet(sessionStoreOverrides.root);
  if (override) return trimTrailingSeparators(override);
  const home = host.homeDir();
  return home ? home + sep + ".codex" : null;
}
function readSessionIndex(path) {
  if (!host.fs.fileExists(path)) return [];
  const contents = host.fs.readFile(path);
  return typeof contents === "string" ? foldSessionIndex(contents) : [];
}
function sessionIndexEntry(path, sessionId) {
  const entries = readSessionIndex(path);
  for (let i = 0; i < entries.length; i++) {
    if (entries[i].id === sessionId) return entries[i];
  }
  return null;
}
var unresolvedCwdDiagnostics = /* @__PURE__ */ new Set();
function readThreadCwd(sessionId) {
  return resolveThreadCwd(sessionId, threadRow(sessionId));
}
function resolveThreadCwd(sessionId, row2) {
  if (!row2) {
    logUnresolvedCwd(sessionId, "no_row");
    return null;
  }
  const cwd = trimmedField(row2, "cwd");
  if (!cwd) {
    logUnresolvedCwd(sessionId, "no_cwd");
    return null;
  }
  if (!host.fs.fileExists(cwd)) {
    logUnresolvedCwd(sessionId, "cwd_absent");
    return null;
  }
  return cwd;
}
function logUnresolvedCwd(sessionId, cause) {
  const key = sessionId + "\0" + cause;
  if (unresolvedCwdDiagnostics.has(key)) return;
  unresolvedCwdDiagnostics.add(key);
  host.log("warn", JSON.stringify({
    provider: "codex",
    event: "session_cwd_unresolved",
    sessionId,
    cause
  }));
}
function sessionTimestamp(value) {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

// plugins/provider-codex/src/launchSession.ts
var MARKER_PREFIX = "codeterm-launch-marker:";
var MIN_MARKER_LENGTH = 16;
var ROLLOUT_HEAD_BYTES = 262144;
var EARLY_RECORDS = 24;
var CANDIDATE_THREADS = 64;
function withLaunchMarker(instructions, marker) {
  if (!marker) return instructions;
  return instructions + "\n\n<!-- " + MARKER_PREFIX + " " + marker + " -->\n";
}
function detectCodexLaunchSession(evidence) {
  const marker = evidence && typeof evidence.launchMarker === "string" ? evidence.launchMarker : "";
  if (marker.length < MIN_MARKER_LENGTH) return null;
  const db = codexStateDbPath();
  if (!db || !host.fs.fileExists(db)) return null;
  const since = Math.floor(Math.max(0, Number(evidence.launchedAtMs) || 0) / 1e3);
  const rows = host.fs.sqliteQuery(
    db,
    "SELECT id, rollout_path FROM threads WHERE created_at >= ? ORDER BY created_at DESC LIMIT " + CANDIDATE_THREADS,
    [since]
  );
  const matches = [];
  for (let i = 0; i < rows.length; i++) {
    const id = typeof rows[i].id === "string" ? rows[i].id : "";
    const path = typeof rows[i].rollout_path === "string" ? rows[i].rollout_path : "";
    if (!id || !path || matches.indexOf(id) !== -1) continue;
    if (rolloutCarriesMarker(path, id, marker)) matches.push(id);
  }
  return matches.length === 1 ? { sessionId: matches[0], source: "launch_marker" } : null;
}
function rolloutCarriesMarker(path, id, marker) {
  const head = host.fs.readFileHead(path, ROLLOUT_HEAD_BYTES);
  if (typeof head !== "string") return false;
  const lines = head.split("\n");
  let records = 0;
  for (let i = 0; i < lines.length && records < EARLY_RECORDS; i++) {
    if (!lines[i].trim()) continue;
    records++;
    let entry;
    try {
      entry = JSON.parse(lines[i]);
    } catch (_) {
      if (records === 1) return false;
      continue;
    }
    const payload = entry && entry.payload;
    if (records === 1) {
      if (entry.type !== "session_meta" || !payload || payload.id !== id) return false;
      continue;
    }
    if (entry.type !== "response_item" || !payload || payload.type !== "message") continue;
    if (payload.role !== "developer" && payload.role !== "user") continue;
    const content = Array.isArray(payload.content) ? payload.content : [];
    for (let j = 0; j < content.length; j++) {
      const text5 = content[j] && content[j].text;
      if (typeof text5 === "string" && text5.indexOf(marker) !== -1) return true;
    }
  }
  return false;
}

// plugins/provider-codex/src/prompt.ts
var DOWN = "G1tC";
var UP = "G1tB";
var ENTER = "DQ==";
var MENU_OPTION_RE = /^[ \t]*([>❯▶➤›])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/;
function parseCodexPrompt(screen) {
  const parsed = parseCodexPromptShape(String(screen || ""));
  return parsed ? parsed.prompt : null;
}
function parseCodexFolderTrustPrompt(screen) {
  const trust = classifyCodexFolderTrustPrompt(screen);
  return trust?.kind === "active" ? trust.prompt : null;
}
function classifyCodexFolderTrustPrompt(screen) {
  const options = activeMenuOptions(screen);
  if (options.length !== 2) return null;
  if (!hasDirectoryContextLine(screen, options[0].line)) return null;
  const affirmativeIndex = options.findIndex((option) => /^(?:yes|trust|allow)\b/i.test(option.label));
  const negativeIndex = options.findIndex((option) => /^(?:no|quit|cancel|decline|deny)\b/i.test(option.label));
  if (affirmativeIndex < 0 || negativeIndex < 0) return null;
  const preamble = cleanPromptText(menuPreamble(screen, options[0].line));
  if (!hasDirectoryTrustContent(preamble)) return null;
  const questionEnd = preamble.indexOf("?");
  const explanation = questionEnd < 0 ? "" : preamble.slice(questionEnd + 1).trim();
  if (explanation.split(/\s+/).filter(Boolean).length < 8) return null;
  if (!hasConfirmFooter(screen, options[options.length - 1].line)) return null;
  const choice = parseActiveTerminalChoice(screen);
  if (!choice || choice.options.length !== 2 || choice.options.filter((option) => option.highlighted).length !== 1) {
    return { kind: "not_on_screen" };
  }
  return {
    kind: "active",
    affirmativeIndex,
    affirmativeSelected: choice.options[affirmativeIndex].highlighted,
    prompt: {
      question: preamble.slice(0, questionEnd + 1).trim(),
      options: choice.options.map((option) => [option.label, option.value]),
      multiSelect: false,
      submitAvailable: false
    }
  };
}
function hasConfirmFooter(screen, lastOptionLine) {
  const lines = normalizeLines(screen);
  let footerLine = -1;
  for (let i = lastOptionLine + 1; i < Math.min(lines.length, lastOptionLine + 5); i++) {
    if (/\benter\b.{0,24}\b(?:confirm|continue|select)\b/i.test(lines[i])) {
      footerLine = i;
      break;
    }
  }
  return footerLine >= 0 && lastMeaningfulLineIndex(lines) === footerLine;
}
function hasDirectoryContextLine(screen, firstOptionLine) {
  const lines = normalizeLines(screen);
  const start = Math.max(0, firstOptionLine - 12);
  const context = lines.slice(start, firstOptionLine).map((line) => line.trim()).filter(Boolean).join(" ");
  const questionEnds = [];
  const questionPattern = /\?/g;
  let questionMatch;
  while ((questionMatch = questionPattern.exec(context)) !== null) questionEnds.push(questionMatch.index);
  for (let i = questionEnds.length - 1; i >= 0; i--) {
    const questionEnd = questionEnds[i];
    const prefix = context.slice(0, questionEnd + 1);
    if (!/\b(folder|directory|workspace)\b/i.test(prefix)) continue;
    if (!/\b(trust|load(?:ed|ing)?)\b/i.test(prefix)) continue;
    if (prefix.split(/\s+/).some((token) => isAbsolutePathToken(token))) return true;
  }
  return false;
}
function hasDirectoryTrustContent(preamble) {
  const content = preamble.toLowerCase();
  if (/\b(?:bypass|disable|override|without restrictions)\b/.test(content)) return false;
  const questionEnd = content.indexOf("?");
  if (questionEnd < 0) return false;
  const question = content.slice(0, questionEnd);
  const explanation = content.slice(questionEnd + 1);
  return /\b(?:folder|directory|workspace)\b/.test(question) && /\b(?:trust(?:ed|ing)?|load(?:ed|ing)?)\b/.test(question) && /\b(?:guidance|rules|config(?:uration)?|settings|policy|policies|instructions|hooks)\b/.test(explanation);
}
function isAbsolutePathToken(token) {
  const path = token.replace(/[),;]+$/, "");
  return /^(?:[a-z]:[\\/]|\\\\|\/\/|\/)/i.test(path);
}
function responseKeysForIndex(optionIndex, screen) {
  const parsed = parseCodexPromptShape(screen);
  if (parsed && !parsed.interactive) {
    const options2 = parsed.prompt.options || [];
    const option = options2[optionIndex];
    const value = Array.isArray(option) ? option[1] : option && option.value;
    return [base64(String(value || optionIndex + 1) + "\r")];
  }
  const options = activeMenuOptions(screen);
  let highlightIdx = 0;
  for (let i = 0; i < options.length; i++) {
    if (options[i].highlighted) {
      highlightIdx = i;
      break;
    }
  }
  const target = optionIndex > 0 ? optionIndex : 0;
  const delta = target - highlightIdx;
  const steps = [];
  if (delta > 0) for (let i = 0; i < delta; i++) steps.push(DOWN);
  else if (delta < 0) for (let i = 0; i < -delta; i++) steps.push(UP);
  steps.push(ENTER);
  return steps;
}
function parseMenuOptions(screen) {
  const lines = normalizeLines(screen);
  const options = [];
  for (let i = 0; i < lines.length; i++) {
    const m = MENU_OPTION_RE.exec(lines[i]);
    if (!m) continue;
    options.push({ highlighted: !!m[1], label: m[3].trim(), value: m[2], line: i });
  }
  return options;
}
function parseActiveTerminalChoice(screen) {
  const lines = normalizeLines(screen);
  const options = activeMenuOptions(screen);
  if (options.length < 2 || !options.some((option) => option.highlighted)) return null;
  const lastOptionLine = options[options.length - 1].line;
  let footerLine = -1;
  for (let i = lastOptionLine + 1; i < Math.min(lines.length, lastOptionLine + 5); i++) {
    if (isActiveChoiceEnterFooter(lines[i])) {
      footerLine = i;
      break;
    }
  }
  if (footerLine === -1 || lastMeaningfulLineIndex(lines) > footerLine) return null;
  return { question: questionForMenu(lines, options[0].line), options };
}
function menuPreamble(screen, firstOptionLine) {
  const lines = normalizeLines(screen);
  let i = firstOptionLine - 1;
  while (i >= 0 && !lines[i].trim()) i--;
  const parts = [];
  while (i >= 0 && lines[i].trim()) {
    parts.unshift(lines[i].trim());
    i--;
  }
  return parts.join(" ");
}
function parseCodexPromptShape(screen) {
  return parseCodexNumberedMenu(screen) || parseCodexLetteredPrompt(screen) || parseCodexNumberedQuestion(screen);
}
function parseCodexNumberedMenu(screen) {
  const lines = normalizeLines(screen);
  const options = activeMenuOptions(screen);
  if (options.length < 2) return null;
  const hasSelector = options.some((o) => o.highlighted);
  const hasFooter = promptFooterNear(lines, options[options.length - 1].line);
  if (!hasSelector && !hasFooter) return null;
  const question = questionForMenu(lines, options[0].line);
  return {
    prompt: { question, options: options.map((o) => [o.label, o.value]) },
    interactive: true
  };
}
function activeMenuOptions(screen) {
  const all = parseMenuOptions(screen);
  if (all.length < 2) return [];
  const cluster = [];
  let prev = all[all.length - 1].line;
  for (let i = all.length - 1; i >= 0; i--) {
    if (prev - all[i].line <= 2 || cluster.length === 0) {
      cluster.unshift(all[i]);
      prev = all[i].line;
    } else {
      break;
    }
  }
  return cluster.length >= 2 ? cluster : [];
}
function promptFooterNear(lines, lastLine) {
  for (let i = lastLine; i < Math.min(lines.length, lastLine + 5); i++) {
    const t = lines[i].toLowerCase();
    if (isActiveChoiceEnterFooter(t)) return true;
    if ((t.indexOf("esc") !== -1 || t.indexOf("enter") !== -1) && (t.indexOf("confirm") !== -1 || t.indexOf("go back") !== -1 || t.indexOf("cancel") !== -1 || t.indexOf("continue") !== -1)) return true;
  }
  return false;
}
function isActiveChoiceEnterFooter(line) {
  return /\benter\b.{0,40}\b(?:confirm|continue|select|authenticate)\b/i.test(line) || /\benter\b.{0,32}\bopen\b.{0,24}\bbrowser\b/i.test(line);
}
function questionForMenu(lines, firstLine) {
  for (let i = firstLine - 1; i >= 0 && firstLine - i <= 5; i--) {
    const t = lines[i].trim();
    if (t.length > 3 && t.charAt(t.length - 1) === "?") return t;
  }
  for (let i = firstLine - 1; i >= 0 && firstLine - i <= 3; i--) {
    const t = lines[i].trim();
    if (t.length > 3) return t;
  }
  return "Permission required";
}
function parseCodexLetteredPrompt(text5) {
  const lines = prosePromptLines(text5);
  const starts = [];
  const optionRe = /^\s*(?:[-*•]\s*)?([A-Z])[.)]\s+(.+?)\s*$/;
  for (let i = 0; i < lines.length; i++) {
    const m = optionRe.exec(lines[i]);
    if (!m || !/[A-C]/.test(m[1])) continue;
    starts.push({ index: i, letter: m[1], label: m[2] });
  }
  if (starts.length < 3) return null;
  return promptFromOptionStarts(lines, starts, "letter");
}
function parseCodexNumberedQuestion(text5) {
  const lines = prosePromptLines(text5);
  const starts = [];
  const optionRe = /^\s*(?:[-*•]\s*)?(\d+)[.)]\s+(.+?)\s*$/;
  for (let i = 0; i < lines.length; i++) {
    const m = optionRe.exec(lines[i]);
    if (!m) continue;
    starts.push({ index: i, letter: m[1], label: m[2] });
  }
  if (starts.length < 2) return null;
  return promptFromOptionStarts(lines, starts, "number");
}
function promptFromOptionStarts(lines, starts, kind) {
  const options = [];
  for (let i = 0; i < starts.length; i++) {
    const start = starts[i];
    const end = i + 1 < starts.length ? starts[i + 1].index : lines.length;
    const parts = [start.label];
    for (let j = start.index + 1; j < end; j++) {
      const t = lines[j].trim();
      if (!t || isCodexFooterLine(t) || /^Step\s+\d+\s+of\s+\d+\b/i.test(t) || t.endsWith("?")) break;
      parts.push(t);
    }
    options.push({ label: cleanPromptText(parts.join(" ")), value: start.letter, highlighted: false });
  }
  const groups = groupOptions(lines, starts, kind);
  if (groups.length === 0) return null;
  const question = groups.length > 1 ? findCompoundTitle(lines, groups[0].questionIndex) || groups[0].question || "Choose an option" : groups[0].question || findQuestion(lines.slice(0, starts[0].index).join("\n")) || "Choose an option";
  const sections = groups.length > 1 ? groups.map((group, i) => ({ label: group.title || "Step " + String(i + 1), active: i === 0 })) : void 0;
  return {
    prompt: {
      question,
      options: options.map((opt) => [opt.label, opt.value]),
      sections,
      multiSelect: false,
      submitAvailable: false
    },
    interactive: false
  };
}
function groupOptions(lines, starts, kind) {
  const firstValue = kind === "letter" ? "A" : "1";
  const groups = [];
  for (let i = 0; i < starts.length; i++) {
    if (starts[i].letter !== firstValue) continue;
    const questionIndex = nearestQuestionIndex(lines, starts[i].index);
    groups.push({
      question: questionIndex >= 0 ? cleanPromptText(lines[questionIndex]) : "",
      questionIndex,
      title: nearestSectionTitle(lines, questionIndex)
    });
  }
  return groups;
}
function normalizeLines(text5) {
  return String(text5 || "").replace(/\r\n?/g, "\n").split("\n");
}
function lastMeaningfulLineIndex(lines) {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line || /^[-─━═]{8,}$/.test(line.replace(/\s/g, ""))) continue;
    return i;
  }
  return -1;
}
function prosePromptLines(text5) {
  const lines = normalizeLines(text5);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (/^\s*•\s+.+\?\s*$/.test(lines[i])) return lines.slice(i);
  }
  return lines;
}
function nearestQuestionIndex(lines, beforeIndex) {
  const start = Math.max(0, beforeIndex - 8);
  for (let i = beforeIndex - 1; i >= start; i--) {
    const t = lines[i].trim();
    if (t.endsWith("?")) return i;
  }
  return -1;
}
function nearestSectionTitle(lines, questionIndex) {
  if (questionIndex < 0) return null;
  for (let i = questionIndex - 1; i >= Math.max(0, questionIndex - 5); i--) {
    const t = cleanPromptText(lines[i]);
    if (!t || t.endsWith("?") || /^[❯>]/.test(t)) continue;
    const step = /^Step\s+\d+\s+of\s+\d+\s+[—-]\s+(.+)$/i.exec(t);
    return step && step[1] ? step[1].trim() : t;
  }
  return null;
}
function findCompoundTitle(lines, beforeQuestionIndex) {
  if (beforeQuestionIndex < 0) return null;
  for (let i = beforeQuestionIndex - 1; i >= Math.max(0, beforeQuestionIndex - 8); i--) {
    const t = cleanPromptText(lines[i]);
    if (!t || /^Step\s+\d+\s+of\s+\d+\b/i.test(t) || t.endsWith("?") || /^[❯>]/.test(t)) continue;
    return t;
  }
  return null;
}
function findQuestion(text5) {
  const lines = normalizeLines(text5).map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].endsWith("?")) return cleanPromptText(lines[i]);
  }
  return lines.length > 0 ? cleanPromptText(lines[lines.length - 1]) : "";
}
function isCodexFooterLine(line) {
  return line.indexOf("ctrl+c to cancel") !== -1 || line.indexOf("tokens used") !== -1 || line.indexOf("\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500") !== -1 || line === "\u276F" || /^›\s*$/.test(line);
}
function cleanPromptText(value) {
  return String(value || "").replace(/^[•⏺]\s*/, "").replace(/─+/g, " ").replace(/\s+/g, " ").trim();
}
function base64(value) {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const bytes = [];
  for (let i = 0; i < value.length; i++) bytes.push(value.charCodeAt(i) & 255);
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i];
    const b = i + 1 < bytes.length ? bytes[i + 1] : 0;
    const c = i + 2 < bytes.length ? bytes[i + 2] : 0;
    out += chars[a >> 2];
    out += chars[(a & 3) << 4 | b >> 4];
    out += i + 1 < bytes.length ? chars[(b & 15) << 2 | c >> 6] : "=";
    out += i + 2 < bytes.length ? chars[c & 63] : "=";
  }
  return out;
}

// plugins/provider-codex/src/usage.ts
var CODEX_USAGE_KIND = "codeterm_usage_v1";
var CODEX_AUTH_CREDENTIAL = "codexAuth";
function fetchCodexUsage(host2) {
  const accountId = stringAt(safeJson(host2.credentialPublic(CODEX_AUTH_CREDENTIAL)), "accountId");
  if (!accountId) return null;
  const response = safeJson(host2.fetch(JSON.stringify({
    url: "https://chatgpt.com/backend-api/wham/usage",
    method: "GET",
    timeoutMs: 6e3,
    headers: {
      "ChatGPT-Account-Id": accountId,
      "User-Agent": "codeterm-usage/1.7.8"
    },
    credential: {
      id: CODEX_AUTH_CREDENTIAL,
      headers: { Authorization: "Bearer {accessToken}" }
    }
  })));
  if (numberAt(response, "status") !== 200) return null;
  const body = safeJson(stringAt(response, "body"));
  const rateLimit = objectAt(body, "rate_limit");
  if (!rateLimit) return null;
  const windows = [rateLimit.primary_window, rateLimit.secondary_window].filter(Boolean);
  const session = windows.find((window) => windowDuration(window) !== null && windowDuration(window) < 86400) ?? null;
  const weekly = windows.find((window) => windowDuration(window) !== null && windowDuration(window) >= 86400) ?? null;
  if (!session && !weekly) return null;
  return JSON.stringify({
    kind: CODEX_USAGE_KIND,
    account_id: accountId,
    session_pct: numberAt(session, "used_percent"),
    session_resets_at_ms: secondsToMs(numberAt(session, "reset_at")),
    weekly_pct: numberAt(weekly, "used_percent"),
    weekly_resets_at_ms: secondsToMs(numberAt(weekly, "reset_at"))
  });
}
function parseCodexUsage(rawText, nowMs) {
  const raw = String(rawText || "");
  if (!raw.trim()) return null;
  const fetched = safeJson(raw);
  if (stringAt(fetched, "kind") === CODEX_USAGE_KIND) {
    return {
      provider: "codex",
      account_id: stringAt(fetched, "account_id"),
      captured_at_ms: nowMs,
      session_pct: numberAt(fetched, "session_pct"),
      session_resets_at_ms: numberAt(fetched, "session_resets_at_ms"),
      weekly_pct: numberAt(fetched, "weekly_pct"),
      weekly_resets_at_ms: numberAt(fetched, "weekly_resets_at_ms"),
      raw_text: raw
    };
  }
  const text5 = raw.split("\r").join("").split("\u2588").join(" ").split("\u2591").join(" ");
  const lines = text5.split("\n");
  const limits = parseCodexLimits(lines, nowMs);
  const session = limits.find((line) => line.family === "general") ?? limits.find((line) => line.family === "legacy-session");
  const weekly = limits.find((line) => line.family === "weekly");
  if (session === void 0 && weekly === void 0) return null;
  return {
    provider: "codex",
    account_id: null,
    captured_at_ms: nowMs,
    session_pct: session ? session.pct : null,
    session_resets_at_ms: session ? session.resetsMs : null,
    weekly_pct: weekly ? weekly.pct : null,
    weekly_resets_at_ms: weekly ? weekly.resetsMs : null,
    raw_text: raw
  };
}
function safeJson(raw) {
  if (typeof raw !== "string" || !raw.trim()) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}
function objectAt(value, key) {
  if (!value || typeof value !== "object") return null;
  const nested = value[key];
  return nested && typeof nested === "object" ? nested : null;
}
function stringAt(value, key) {
  if (!value || typeof value !== "object") return null;
  const nested = value[key];
  return typeof nested === "string" && nested ? nested : null;
}
function numberAt(value, key) {
  if (!value || typeof value !== "object") return null;
  const nested = value[key];
  return typeof nested === "number" && Number.isFinite(nested) ? nested : null;
}
function secondsToMs(value) {
  return value === null ? null : value * 1e3;
}
function windowDuration(window) {
  return numberAt(window, "limit_window_seconds");
}
var CODEX_BORDER_RE = /^[\s─-╿]+|[\s─-╿]+$/g;
function stripCodexBorder(raw) {
  return String(raw || "").replace(CODEX_BORDER_RE, "");
}
var CODEX_HEADER_RE = /\blimit\s*:?\s*$/i;
function isCodexSectionHeader(line) {
  if (!line || line.indexOf("%") !== -1) return false;
  return CODEX_HEADER_RE.test(line);
}
function parseCodexLimits(lines, nowMs) {
  const out = [];
  let sectionHeader = null;
  for (let i = 0; i < lines.length; i++) {
    const line = stripCodexBorder(lines[i]);
    if (!line) continue;
    if (isCodexSectionHeader(line)) {
      sectionHeader = line;
      continue;
    }
    let context = line;
    for (let j = i - 1; j >= 0; j--) {
      const prev = stripCodexBorder(lines[j]);
      if (prev) {
        context = `${prev} ${line}`;
        break;
      }
    }
    const gauge = parseCodexGaugeLine(line, context, nowMs);
    if (gauge.pct === null && gauge.resetsMs === null) continue;
    const family = codexLimitFamily(context, sectionHeader);
    if (family !== "model") {
      out.push({ family, pct: gauge.pct, resetsMs: gauge.resetsMs });
    }
  }
  return out;
}
function parseCodexGaugeLine(line, context, nowMs) {
  const re = /(\d+(?:\.\d+)?)\s*%\s*(left|remaining|used)\s*\(\s*resets?\s+(?:at\s+)?([^)]+?)\)/i;
  const caps = re.exec(line);
  if (!caps) {
    return { pct: null, resetsMs: null };
  }
  const value = parseFloat(caps[1]);
  const pct = codexPctToUsed(value, (caps[2] || "").toLowerCase());
  const resetsRaw = (caps[3] || "").trim();
  const isWeekly = /\bweekly\b/i.test(context);
  const resetsMs = isWeekly ? codexParseWeeklyResets(resetsRaw, nowMs) : codexParseSessionResets(resetsRaw, nowMs);
  return { pct, resetsMs };
}
function codexPctToUsed(value, mode) {
  if (isNaN(value)) return null;
  const rawMode = mode === "used" ? "used" : "left";
  const pct = rawMode === "used" ? value : 100 - value;
  return Math.min(100, Math.max(0, pct));
}
var CODEX_MODEL_NAME_RE = /\b(?:gpt|opus|haiku|sonnet|claude|codex|o\d|gemini)\b.*\d+\.\d+/i;
function codexLimitFamily(text5, sectionHeader) {
  const lower = String(text5 || "").toLowerCase();
  const hasGeneral = /\bgeneral\b/.test(lower);
  const hasWeekly = /\bweekly\b/.test(lower);
  const hasSession = /\b5(?:\s*|-\s*)(?:h|hour|hours)\b/.test(lower);
  const hasModel = CODEX_MODEL_NAME_RE.test(lower) || sectionHeader !== null && CODEX_MODEL_NAME_RE.test(sectionHeader);
  if (hasModel) return "model";
  if (hasGeneral) return hasWeekly ? "weekly" : "general";
  if (hasWeekly) return "weekly";
  if (hasSession) return "legacy-session";
  return "general";
}
function codexParseSessionResets(raw, nowMs) {
  const m = /(\d{1,2}):(\d{2})/.exec(String(raw || "").trim());
  if (!m) return null;
  const hh = parseInt(m[1], 10);
  const mm = parseInt(m[2], 10);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hh, mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1e3);
  return cand.getTime();
}
var CODEX_MONTHS = {
  jan: 0,
  january: 0,
  feb: 1,
  february: 1,
  mar: 2,
  march: 2,
  apr: 3,
  april: 3,
  may: 4,
  jun: 5,
  june: 5,
  jul: 6,
  july: 6,
  aug: 7,
  august: 7,
  sep: 8,
  sept: 8,
  september: 8,
  oct: 9,
  october: 9,
  nov: 10,
  november: 10,
  dec: 11,
  december: 11
};
function codexParseWeeklyResets(raw, nowMs) {
  const s = String(raw || "").trim().replace(/^at\s+/i, "");
  let hh;
  let mm;
  let day;
  let month;
  let year = null;
  let m = /(\d{1,2}):(\d{2})\s+on\s+(\d{1,2})\s+([A-Za-z]+)(?:,?\s+(\d{4}))?/i.exec(s);
  if (m) {
    hh = parseInt(m[1], 10);
    mm = parseInt(m[2], 10);
    day = parseInt(m[3], 10);
    month = CODEX_MONTHS[m[4].toLowerCase()];
    year = m[5] ? parseInt(m[5], 10) : null;
  } else {
    m = /(\d{1,2}):(\d{2})\s+on\s+([A-Za-z]+)\s+(\d{1,2})(?:,?\s+(\d{4}))?/i.exec(s);
    if (!m) return null;
    hh = parseInt(m[1], 10);
    mm = parseInt(m[2], 10);
    month = CODEX_MONTHS[m[3].toLowerCase()];
    day = parseInt(m[4], 10);
    year = m[5] ? parseInt(m[5], 10) : null;
  }
  if (month === void 0) return null;
  const now = new Date(nowMs);
  let cand = new Date(year ?? now.getFullYear(), month, day, hh, mm, 0, 0);
  if (year === null && cand.getTime() <= nowMs) {
    cand = new Date(now.getFullYear() + 1, month, day, hh, mm, 0, 0);
  }
  return cand.getTime();
}

// plugins/provider-codex/src/execWrapper.ts
var MAX_SUMMARY_CHARS = 160;
function quotedValue(source, key) {
  const keyMatch = new RegExp("\\b" + key + "\\s*:\\s*").exec(source);
  if (!keyMatch) return null;
  const quoteIndex = keyMatch.index + keyMatch[0].length;
  const quote = source[quoteIndex];
  if (quote !== '"' && quote !== "'") return null;
  let value = "";
  for (let i = quoteIndex + 1; i < source.length; i++) {
    const char = source[i];
    if (char === quote) return decodeString(value, quote);
    if (char === "\\" && i + 1 < source.length) {
      value += char + source[++i];
    } else {
      value += char;
    }
  }
  return null;
}
function decodeString(value, quote) {
  return value.replace(/\\(.)/g, (_match, escaped) => {
    if (escaped === "n") return "\n";
    if (escaped === "r") return "\r";
    if (escaped === "t") return "	";
    if (escaped === "\\" || escaped === '"' || quote === "'" && escaped === "'") return escaped;
    return "\\" + escaped;
  });
}
function callObjectAt(source, start) {
  const open = start < 0 ? -1 : source.indexOf("{", start);
  if (open < 0) return null;
  let depth = 0;
  let quote = "";
  for (let i = open; i < source.length; i++) {
    const char = source[i];
    if (quote) {
      if (char === "\\") i++;
      else if (char === quote) quote = "";
    } else if (char === '"' || char === "'") {
      quote = char;
    } else if (char === "{") {
      depth++;
    } else if (char === "}" && --depth === 0) {
      return source.substring(open + 1, i);
    }
  }
  return null;
}
function callObjects(source, toolName) {
  const objects = [];
  const needle = `tools.${toolName}`;
  let offset = 0;
  while (offset < source.length) {
    const start = source.indexOf(needle, offset);
    if (start < 0) break;
    const body = callObjectAt(source, start);
    if (body) objects.push(body);
    offset = start + needle.length;
  }
  return objects;
}
function scriptCommands(raw) {
  const commandBodies = [
    ...callObjects(raw, "exec_command"),
    ...callObjects(raw, "shell_command")
  ];
  return commandBodies.map((body) => quotedValue(body, "cmd") || quotedValue(body, "command") || "").filter(Boolean);
}
function commandText(value) {
  if (typeof value === "string") return value;
  if (!Array.isArray(value) || value.length === 0 || !value.every((argument) => typeof argument === "string")) return "";
  const argv = value;
  if (!argv[0].trim()) return "";
  return argv.map((argument) => argument === "" || /[\s\"]/.test(argument) ? JSON.stringify(argument) : argument).join(" ");
}
function unwrapCommandExecution(value) {
  if (typeof value === "string") return value.trim() ? { command: value, detail: value } : null;
  if (!Array.isArray(value) || value.length < 3 || !value.every((argument) => typeof argument === "string")) return null;
  const argv = value;
  for (let i = 1; i < argv.length - 1; i++) {
    const option = argv[i].toLowerCase();
    if (option !== "-command" && option !== "-c" && option !== "-lc" && option !== "/c") continue;
    if (!argv[i + 1].trim()) return null;
    return { command: argv[i + 1], detail: commandText(argv) };
  }
  return null;
}
function execScriptCommands(raw) {
  return typeof raw === "string" ? scriptCommands(raw) : [];
}
function jsonCallObject(source) {
  const match = /\btools\.shell_command\(\s*(\{[\s\S]*\})\s*\)\s*;/.exec(source);
  if (!match) return null;
  try {
    const value = JSON.parse(match[1]);
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  } catch (_) {
    return null;
  }
}
function unwrapShellCommand(raw) {
  if (typeof raw !== "string") return null;
  try {
    const value = jsonCallObject(raw);
    if (value) {
      const command2 = typeof value.command === "string" ? value.command : typeof value.cmd === "string" ? value.cmd : "";
      if (!command2.trim()) return null;
      const workdir2 = typeof value.workdir === "string" && value.workdir ? value.workdir : void 0;
      const timeout_ms2 = typeof value.timeout_ms === "number" ? value.timeout_ms : void 0;
      return { command: command2, ...workdir2 ? { workdir: workdir2 } : {}, ...timeout_ms2 !== void 0 ? { timeout_ms: timeout_ms2 } : {} };
    }
    const body = callObjects(raw, "shell_command")[0];
    if (!body) return null;
    const command = quotedValue(body, "command") || quotedValue(body, "cmd");
    if (!command || !command.trim()) return null;
    const workdir = quotedValue(body, "workdir") || void 0;
    const timeoutText = /\btimeout_ms\s*:\s*(\d+)/.exec(body);
    const timeout_ms = timeoutText ? Number(timeoutText[1]) : void 0;
    return { command, ...workdir ? { workdir } : {}, ...timeout_ms !== void 0 ? { timeout_ms } : {} };
  } catch (_) {
    return null;
  }
}
function compactCommand(command) {
  return command.replace(/\s+/g, " ").trim();
}
function clipSummary(value, limit = MAX_SUMMARY_CHARS) {
  if (value.length <= limit) return value;
  return value.substring(0, Math.max(1, limit - 3)).trimEnd() + "...";
}
function commandSummary(commands) {
  const compact = commands.map(compactCommand).filter(Boolean);
  const first = compact[0];
  const suffix = compact.length > 1 ? ` (+${compact.length - 1} more)` : "";
  const summary = clipSummary(first, MAX_SUMMARY_CHARS - suffix.length) + suffix;
  return { kind: "command", summary, detail: commands.map((value) => value.trim()).join("\n\n") };
}
function readableToolName(name) {
  const leaf = name.split("__").pop() || name;
  return leaf.replace(/_/g, " ").replace(/\b\w/g, (char) => char.toUpperCase());
}
function fallbackSummary(raw) {
  const nested = /\btools\.([A-Za-z_$][\w$]*)/.exec(raw);
  const firstLine = raw.split(/\r\n?|\n/).map((line) => line.trim()).find(Boolean) || "Tool activity";
  const summary = nested ? readableToolName(nested[1]) : firstLine;
  return { kind: "generic", summary: clipSummary(summary), detail: raw.trim() };
}
function summarizeExecScript(raw) {
  if (typeof raw !== "string" || !raw.trim()) return null;
  const commands = scriptCommands(raw);
  if (commands.length > 0) return commandSummary(commands);
  const image = callObjects(raw, "view_image")[0];
  if (image) {
    const path = quotedValue(image, "path");
    if (path) return { kind: "read", summary: clipSummary(path), detail: path };
  }
  const stdin = callObjects(raw, "write_stdin")[0];
  if (stdin) {
    const chars = quotedValue(stdin, "chars");
    const summary = chars ? "Send input to running command" : "Wait for command output";
    return { kind: "generic", summary, detail: summary };
  }
  return fallbackSummary(raw);
}

// plugins/provider-codex/src/patchEdits.ts
function patchEdits(patch) {
  if (typeof patch !== "string" || patch.indexOf("*** Begin Patch") < 0) return [];
  try {
    const edits = [];
    let path = null;
    let oldLines = [];
    let newLines = [];
    const flush = () => {
      if (path && (oldLines.length > 0 || newLines.length > 0)) edits.push({ path, old: oldLines.join("\n"), new: newLines.join("\n") });
      oldLines = [];
      newLines = [];
    };
    for (const line of patch.replace(/\r\n/g, "\n").split("\n")) {
      const header = /^\*\*\* (?:Update|Add|Delete) File: (.+)$/.exec(line);
      if (header) {
        flush();
        path = header[1].trim();
      } else if (line === "@@" || line.indexOf("@@ ") === 0) {
        flush();
      } else if (path && line.indexOf("-") === 0) {
        oldLines.push(line.substring(1));
      } else if (path && line.indexOf("+") === 0) {
        newLines.push(line.substring(1));
      }
    }
    flush();
    return edits;
  } catch (_) {
    return [];
  }
}

// plugins/provider-codex/src/actionTitle.ts
var MAX_TITLE_CHARS = 48;
function clip(value) {
  const compact = value.replace(/\s+/g, " ").trim();
  if (compact.length <= MAX_TITLE_CHARS) return compact;
  return compact.substring(0, MAX_TITLE_CHARS - 3).trimEnd() + "...";
}
function text(value) {
  return typeof value === "string" ? value : "";
}
function baseName(path) {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() || trimmed;
}
function commandHead(command) {
  const tokens = command.replace(/\s+/g, " ").trim().split(" ").filter(Boolean);
  const width = tokens.length > 1 && tokens[1].charAt(0) !== "-" && tokens[1].charAt(0) !== '"' ? 2 : 1;
  return tokens.slice(0, width).join(" ");
}
function actionTitle(fields) {
  const summary = text(fields.tool_summary);
  const detail = text(fields.tool_detail);
  if (fields.tool_kind === "command") {
    const more = /\(\+(\d+) more\)$/.exec(summary);
    return clip(commandHead(detail || summary) + (more ? ` +${more[1]}` : ""));
  }
  if (fields.tool_kind === "read" && summary) return clip(`Read ${baseName(summary)}`);
  if (fields.tool_kind === "edit") {
    const edits = Array.isArray(fields.tool_edits) ? fields.tool_edits : [];
    const paths = edits.map((edit) => text(edit && edit.path)).filter((path, index, all) => path && all.indexOf(path) === index);
    if (paths.length > 0) return clip(`Patched ${baseName(paths[0])}` + (paths.length > 1 ? ` +${paths.length - 1}` : ""));
  }
  return clip(summary || "Tool activity");
}

// plugins/provider-codex/src/agentActivity.ts
var AGENT_STATES = {
  started: "started",
  pending_init: "started",
  running: "running",
  completed: "completed",
  errored: "failed",
  failed: "failed",
  interrupted: "interrupted",
  shutdown: "closed",
  closed: "closed"
};
function text2(value) {
  return typeof value === "string" ? value.trim() : "";
}
function record(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : null;
}
function row(uuid, blocks, ts, timestamp) {
  if (blocks.length === 0) return null;
  return { role: "assistant", ...uuid ? { uuid } : {}, blocks, ts, timestamp };
}
function agentBlock(fields, summary, turnId) {
  const input = {};
  for (const key in fields) if (fields[key]) input[key] = fields[key];
  const flowId = text2(turnId);
  return { kind: "tool_use", data: {
    name: "agent_activity",
    input,
    tool_kind: "agent",
    tool_summary: summary,
    tool_status: "success",
    ...flowId ? { tool_flow_id: flowId } : {}
  } };
}
function thoughtRow(id, summaries, ts, timestamp) {
  const blocks = summaries.filter((summary) => typeof summary === "string" && summary.trim() !== "").map((summary) => ({ kind: "thinking", data: { text: summary } }));
  return row(text2(id) ? `reasoning-${text2(id)}` : "", blocks, ts, timestamp);
}
function reasoningItemRow(item, ts, timestamp) {
  return thoughtRow(item.id, Array.isArray(item.summary_text) ? item.summary_text : [], ts, timestamp);
}
function reasoningResponseRow(payload, ts, timestamp) {
  const parts = Array.isArray(payload.summary) ? payload.summary : [];
  const summaries = parts.map((part) => {
    const entry = record(part);
    return entry && entry.type === "summary_text" ? entry.text : void 0;
  });
  return thoughtRow(payload.id, summaries, ts, timestamp);
}
function subAgentRow(item, id, turnId, ts, timestamp) {
  const agentId = text2(item.agent_thread_id);
  const agentPath = text2(item.agent_path);
  if (!agentId && !agentPath) return null;
  const state = AGENT_STATES[text2(item.kind)] ?? "unknown";
  const block = agentBlock({ agent_id: agentId, agent_path: agentPath, agent_state: state }, agentPath || agentId, turnId);
  return row(text2(id) ? `agent-${text2(id)}` : "", [block], ts, timestamp);
}
function collabCallRow(item, turnId, ts, timestamp) {
  const receivers = Array.isArray(item.receiver_thread_ids) ? item.receiver_thread_ids : [];
  const agents = Array.isArray(item.receiver_agents) ? item.receiver_agents.map(record) : [];
  const states = record(item.agents_states);
  const seen = [];
  const blocks = [];
  for (const receiver of receivers) {
    const agentId = text2(receiver);
    if (!agentId || seen.indexOf(agentId) !== -1) continue;
    seen.push(agentId);
    const agentPath = text2(agents.find((entry) => entry !== null && text2(entry.thread_id) === agentId)?.agent_path);
    const state = AGENT_STATES[text2(record(states?.[agentId])?.status)] ?? "unknown";
    blocks.push(agentBlock({ agent_id: agentId, agent_path: agentPath, agent_state: state }, agentPath || agentId, turnId));
  }
  return row(text2(item.id) ? `collab-${text2(item.id)}` : "", blocks, ts, timestamp);
}
function agentMessageRow(payload, ts, timestamp) {
  const from = text2(payload.author);
  const to = text2(payload.recipient);
  if (!from || !to || from === to) return null;
  const turnId = record(payload.internal_chat_message_metadata_passthrough)?.turn_id;
  const block = agentBlock({ message_from: from, message_to: to }, `${from} \u2192 ${to}`, turnId);
  return row(text2(payload.id) ? `agent-message-${text2(payload.id)}` : "", [block], ts, timestamp);
}

// plugins/provider-codex/src/fileDiffs.ts
var LINE_MARKERS = " +-\\";
var HUNK_HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;
function splitLines(text5) {
  const lines = text5.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return lines;
}
function contentHunks(content, marker) {
  const lines = content ? splitLines(content) : [];
  if (lines.length === 0) return [];
  const added = marker === "+";
  return [{
    oldStart: added ? 0 : 1,
    oldLines: added ? 0 : lines.length,
    newStart: added ? 1 : 0,
    newLines: added ? lines.length : 0,
    lines: lines.map((line) => marker + line)
  }];
}
function unifiedHunks(diff) {
  const hunks = [];
  let current = null;
  for (const line of splitLines(diff)) {
    const header = HUNK_HEADER.exec(line);
    if (header) {
      current = {
        oldStart: Number(header[1]),
        oldLines: header[2] === void 0 ? 1 : Number(header[2]),
        newStart: Number(header[3]),
        newLines: header[4] === void 0 ? 1 : Number(header[4]),
        lines: []
      };
      hunks.push(current);
    } else if (!current) {
      if (line.indexOf("--- ") !== 0 && line.indexOf("+++ ") !== 0) return null;
    } else if (!line || LINE_MARKERS.indexOf(line.charAt(0)) < 0) {
      return null;
    } else {
      current.lines.push(line);
    }
  }
  return hunks.length > 0 ? hunks : null;
}
function fileDiff(path, hunks, movePath) {
  let additions = 0;
  let deletions = 0;
  for (const hunk of hunks) {
    for (const line of hunk.lines) {
      if (line.charAt(0) === "+") additions++;
      else if (line.charAt(0) === "-") deletions++;
    }
  }
  return movePath ? { path, movePath, additions, deletions, hunks } : { path, additions, deletions, hunks };
}
function changeDiff(path, value) {
  if (!path.trim() || !value || typeof value !== "object" || Array.isArray(value)) return null;
  const change = value;
  if ((change.type === "add" || change.type === "delete") && typeof change.content === "string") {
    return fileDiff(path, contentHunks(change.content, change.type === "add" ? "+" : "-"), "");
  }
  if (change.type !== "update" || typeof change.unified_diff !== "string") return null;
  const hunks = unifiedHunks(change.unified_diff);
  if (!hunks) return null;
  return fileDiff(path, hunks, typeof change.move_path === "string" && change.move_path.trim() ? change.move_path : "");
}
function changedPaths(changes) {
  if (!changes || typeof changes !== "object" || Array.isArray(changes)) return [];
  return Object.keys(changes).filter((path) => path.trim().length > 0);
}
function codexFileDiffs(changes) {
  const diffs = [];
  for (const path of changedPaths(changes)) {
    const diff = changeDiff(path, changes[path]);
    if (diff) diffs.push(diff);
  }
  return diffs;
}
function diffEdits(diffs) {
  const edits = [];
  for (const diff of diffs) {
    for (const hunk of diff.hunks) {
      const oldLines = hunk.lines.filter((line) => line.charAt(0) === "-").map((line) => line.substring(1));
      const newLines = hunk.lines.filter((line) => line.charAt(0) === "+").map((line) => line.substring(1));
      if (oldLines.length > 0 || newLines.length > 0) edits.push({ path: diff.path, old: oldLines.join("\n"), new: newLines.join("\n") });
    }
  }
  return edits;
}

// plugins/provider-codex/src/richResults.ts
function text3(value) {
  return typeof value === "string" ? value : "";
}
function record2(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : null;
}
function webFields(source) {
  const action = record2(source.action);
  const rich = { kind: "web" };
  const query = text3(source.query) || text3(action?.query);
  if (query) rich.query = query;
  const actionName = text3(source.action) || text3(action?.type);
  if (actionName) rich.action = actionName;
  if (text3(action?.url)) rich.url = text3(action?.url);
  if (text3(action?.pattern)) rich.pattern = text3(action?.pattern);
  if (Array.isArray(source.results)) rich.resultCount = source.results.length;
  return rich;
}
function resultMessage(uuid, id, name, rich, summary, detail, failed, turnId, ts, timestamp) {
  return {
    role: "assistant",
    uuid,
    ts,
    timestamp,
    blocks: [{ kind: "tool_use", data: {
      id,
      name,
      tool_kind: rich.kind,
      tool_summary: summary,
      tool_title: summary,
      ...detail ? { tool_detail: detail } : {},
      tool_rich_result: rich,
      ...text3(turnId) ? { tool_flow_id: text3(turnId) } : {},
      tool_status: failed ? "error" : "success"
    } }]
  };
}
function webSearchEndResult(payload, ts, timestamp) {
  const source = record2(payload);
  const id = text3(source?.call_id);
  if (!source || !id) return null;
  const rich = webFields(source);
  const summary = rich.query || rich.url || "";
  if (!summary) return null;
  return resultMessage(`result-${id}`, id, "web_search", rich, summary, summary, false, source.turn_id, ts, timestamp);
}
function imageGenerationEndResult(payload, ts, timestamp) {
  const source = record2(payload);
  const id = text3(source?.call_id);
  if (!source || !id) return null;
  const rich = { kind: "image", origin: "generated" };
  const status = text3(source.status);
  const path = text3(source.saved_path);
  const prompt = text3(source.revised_prompt);
  if (status) rich.status = status;
  if (path) rich.path = path;
  if (prompt) rich.prompt = prompt;
  const summary = path && baseName(path) || prompt || status || "image generation";
  return resultMessage(`result-${id}`, id, "image_generation", rich, summary, path || prompt, status === "failed" || status === "error", source.turn_id, ts, timestamp);
}
function imageViewResult(item, ts, timestamp, turnId) {
  const id = text3(item.id);
  const path = text3(item.path);
  if (!id || !path) return null;
  const rich = { kind: "image", origin: "viewed", path };
  return resultMessage(`activity-${id}`, id, "image_view", rich, baseName(path) || path, path, false, turnId, ts, timestamp);
}
function extensionResult(item, ts, timestamp, turnId) {
  const id = text3(item.id);
  if (!id || !text3(item.kind).startsWith("web.")) return null;
  const rich = webFields(item);
  const summary = rich.query || rich.url || rich.action || "";
  if (!summary) return null;
  return resultMessage(`activity-${id}`, id, "extension", rich, summary, summary, false, turnId, ts, timestamp);
}

// plugins/provider-codex/src/activity.ts
function parsedReadTitle(value) {
  if (!Array.isArray(value) || value.length === 0) return "";
  const names = [];
  for (const entry of value) {
    if (!entry || typeof entry !== "object") return "";
    const parsed = entry;
    const name = text4(parsed.name) || baseName(text4(parsed.path));
    if (parsed.type !== "read" || !name) return "";
    if (names.indexOf(name) === -1) names.push(name);
  }
  return actionTitle({ tool_summary: `Read ${names[0]}` + (names.length > 1 ? ` +${names.length - 1}` : "") });
}
function text4(value) {
  return typeof value === "string" ? value : "";
}
function exactOutcomeNumber(value, nonnegative) {
  if (typeof value !== "number" || !Number.isFinite(value) || !Number.isSafeInteger(value)) return void 0;
  if (nonnegative && value < 0) return void 0;
  return value;
}
function durationMilliseconds(item) {
  const numericDuration = exactOutcomeNumber(item.duration_ms, true);
  if (numericDuration !== void 0) return numericDuration;
  if (!item.duration || typeof item.duration !== "object" || Array.isArray(item.duration)) return void 0;
  const duration = item.duration;
  const secs = exactOutcomeNumber(duration.secs, true);
  const nanos = exactOutcomeNumber(duration.nanos, true);
  if (secs === void 0 || nanos === void 0) return void 0;
  return exactOutcomeNumber(secs * 1e3 + Math.round(nanos / 1e6), true);
}
function commandOutcome(item) {
  const outcome2 = {};
  const exitCode = exactOutcomeNumber(item.exit_code, false);
  const durationMs = durationMilliseconds(item);
  if (exitCode !== void 0) outcome2.tool_exit_code = exitCode;
  if (durationMs !== void 0) outcome2.tool_duration_ms = durationMs;
  return outcome2;
}
function completedActivity(value, ts, timestamp, turnId) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const item = value;
  const id = text4(item.id);
  if (!id) return null;
  const flow = text4(turnId) ? { tool_flow_id: text4(turnId) } : {};
  const message = { role: "assistant", uuid: `activity-${id}`, blocks: [], ts, timestamp };
  if (item.type === "CommandExecution") {
    const command = commandText(item.command);
    if (!command || item.status !== "completed" && item.status !== "failed") return null;
    const exitCode = exactOutcomeNumber(item.exit_code, false);
    const failed = item.status === "failed" || exitCode !== void 0 && exitCode !== 0;
    const output = text4(item.aggregated_output) || [text4(item.stdout), text4(item.stderr)].filter(Boolean).join("\n");
    const fields2 = { tool_kind: "command", tool_summary: command, tool_detail: command };
    const outcome2 = commandOutcome(item);
    message.blocks = [
      { kind: "tool_use", data: {
        id,
        name: "command_execution",
        input: { command, ...Array.isArray(item.command) ? { argv: item.command } : {} },
        ...fields2,
        tool_echo: true,
        tool_title: parsedReadTitle(item.parsed_cmd) || actionTitle(fields2),
        ...outcome2,
        ...flow,
        tool_status: failed ? "error" : "success"
      } },
      { kind: "tool_result", data: { tool_use_id: id, content: output, is_error: failed, ...outcome2 } }
    ];
    return message;
  }
  if (item.type === "FileChange") {
    const paths = changedPaths(item.changes);
    if (paths.length === 0 || item.status !== "completed" && item.status !== "failed") return null;
    const failed = item.status === "failed";
    const diffs = codexFileDiffs(item.changes);
    const fields2 = {
      tool_kind: "edit",
      tool_summary: paths[0] + (paths.length > 1 ? ` +${paths.length - 1}` : ""),
      tool_detail: paths.join("\n"),
      tool_edits: diffEdits(diffs)
    };
    message.blocks = [
      { kind: "tool_use", data: {
        id,
        name: "file_change",
        input: { paths },
        ...fields2,
        tool_title: actionTitle({ ...fields2, tool_edits: paths.map((path) => ({ path })) }),
        ...flow,
        ...diffs.length > 0 ? { file_diffs: diffs } : {},
        tool_status: failed ? "error" : "success"
      } },
      { kind: "tool_result", data: { tool_use_id: id, content: [text4(item.stdout), text4(item.stderr)].filter(Boolean).join("\n"), is_error: failed } }
    ];
    return message;
  }
  if (item.type === "ImageView") return imageViewResult(value, ts, timestamp, turnId);
  if (item.type === "Extension") return extensionResult(value, ts, timestamp, turnId);
  const fields = value;
  if (item.type === "SubAgentActivity") return subAgentRow(fields, id, turnId, ts, timestamp);
  if (item.type === "CollabAgentToolCall") return collabCallRow(fields, turnId, ts, timestamp);
  if (item.type === "Reasoning") return reasoningItemRow(fields, ts, timestamp);
  return null;
}

// plugins/provider-codex/src/trust-carryover.ts
function normalizedPath(value) {
  const isWindowsPath = /^[A-Za-z]:[\\/]/.test(value) || value.startsWith("\\\\") || value.startsWith("//");
  return isWindowsPath ? value.replace(/\//g, "\\").toLowerCase() : value;
}
function isCodeTermProvisionedWorkspacePath(value) {
  const parts = normalizedPath(value).split(/[\\/]+/).filter(Boolean);
  return parts.some(
    (part, index) => /^\.codeterm(?:-dev(?:-[a-z0-9-]+)?)?$/.test(part) && parts[index + 1] === "provisioned-workspaces"
  );
}
function parseString(value) {
  const quote = value[0];
  if (quote !== "'" && quote !== '"') return null;
  for (let i = 1; i < value.length; i++) {
    if (quote === '"' && value[i] === "\\") {
      i++;
      continue;
    }
    if (value[i] !== quote) continue;
    const raw = value.slice(0, i + 1);
    try {
      return { value: quote === '"' ? JSON.parse(raw) : raw.slice(1, -1), next: i + 1 };
    } catch {
      return null;
    }
  }
  return null;
}
function parseDottedKey(value) {
  const parts = [];
  let index = 0;
  while (index < value.length) {
    while (/\s/.test(value[index] || "")) index++;
    if (index >= value.length) break;
    let part;
    if (value[index] === "'" || value[index] === '"') {
      const parsed = parseString(value.slice(index));
      if (!parsed) return null;
      part = parsed.value;
      index += parsed.next;
    } else {
      const match = /^[A-Za-z0-9_-]+/.exec(value.slice(index));
      if (!match) return null;
      part = match[0];
      index += part.length;
    }
    parts.push(part);
    while (/\s/.test(value[index] || "")) index++;
    if (index === value.length) return parts;
    if (value[index] !== ".") return null;
    index++;
  }
  return parts.length ? parts : null;
}
function withoutComment(value) {
  let quote = "";
  for (let i = 0; i < value.length; i++) {
    const char = value[i];
    if (quote === '"' && char === "\\") {
      i++;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      continue;
    }
    if (char === "'" || char === '"') quote = char;
    else if (char === "#") return value.slice(0, i);
  }
  return value;
}
function headerParts(line) {
  const value = withoutComment(line).trim();
  if (!value.startsWith("[") || !value.endsWith("]") || value.startsWith("[[")) return null;
  return parseDottedKey(value.slice(1, -1));
}
function assignmentParts(line) {
  const value = withoutComment(line);
  let quote = "";
  for (let i = 0; i < value.length; i++) {
    const char = value[i];
    if (quote === '"' && char === "\\") {
      i++;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      continue;
    }
    if (char === "'" || char === '"') quote = char;
    else if (char === "=") {
      const key = parseDottedKey(value.slice(0, i));
      return key ? { key, value: value.slice(i + 1).trim() } : null;
    }
  }
  return null;
}
function parseTrustValue(value) {
  const parsed = parseString(value.trim());
  if (!parsed || parsed.next !== value.trim().length) return null;
  return parsed.value === "trusted" || parsed.value === "untrusted" ? parsed.value : null;
}
function projectTablePath(parts) {
  return parts?.length === 2 && parts[0] === "projects" ? parts[1] : null;
}
function collectProfileEntries(profile) {
  const lines = profile.split(/\r?\n/);
  const entries = /* @__PURE__ */ new Map();
  const seenProjectPaths = /* @__PURE__ */ new Set();
  let activePath = null;
  let activeHeader = "";
  let trust = null;
  let trustSeen = false;
  const flush = () => {
    if (activePath === null || !trust) return true;
    const key = normalizedPath(activePath);
    if (entries.has(key)) return false;
    if (!isCodeTermProvisionedWorkspacePath(activePath)) {
      entries.set(key, { path: activePath, header: activeHeader, assignment: trust.line });
    }
    return true;
  };
  for (const line of lines) {
    const parts = headerParts(line);
    if (parts) {
      if (!flush()) return null;
      activePath = projectTablePath(parts);
      activeHeader = activePath === null ? "" : withoutComment(line).trim();
      if (activePath !== null) {
        const key = normalizedPath(activePath);
        if (seenProjectPaths.has(key)) return null;
        seenProjectPaths.add(key);
      }
      trust = null;
      trustSeen = false;
      continue;
    }
    const malformedHeader = withoutComment(line).trim();
    if (malformedHeader.startsWith("[")) {
      if (/^\[\[\s*projects(?:\s*\.|\s*\])/.test(malformedHeader) || /^\[\s*projects(?:\s*\.|\s*\])/.test(malformedHeader)) return null;
      if (!flush()) return null;
      activePath = null;
      activeHeader = "";
      trust = null;
      trustSeen = false;
      continue;
    }
    if (activePath === null) continue;
    const assignment = assignmentParts(line);
    if (!assignment || assignment.key.length !== 1 || assignment.key[0] !== "trust_level") continue;
    if (trustSeen) return null;
    trustSeen = true;
    const parsed = parseTrustValue(assignment.value);
    if (parsed) trust = { line: line.replace(/\r$/, "") };
  }
  return flush() ? [...entries.values()] : null;
}
function inlineTableKeys(value) {
  const trimmed = value.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  const body = trimmed.slice(1, -1);
  const keys = [];
  let start = 0;
  let quote = "";
  let depth = 0;
  const add = (segment) => {
    if (!segment.trim()) return true;
    const assignment = assignmentParts(segment);
    if (!assignment || !assignment.key.length) return false;
    keys.push(assignment.key[0]);
    return true;
  };
  for (let i = 0; i < body.length; i++) {
    const char = body[i];
    if (quote === '"' && char === "\\") {
      i++;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      continue;
    }
    if (char === "'" || char === '"') quote = char;
    else if (char === "{" || char === "[") depth++;
    else if (char === "}" || char === "]") depth--;
    else if (char === "," && depth === 0) {
      if (!add(body.slice(start, i))) return null;
      start = i + 1;
    }
  }
  if (!add(body.slice(start))) return null;
  return keys;
}
function collectExistingPaths(config) {
  const lines = config.split(/\r?\n/);
  const paths = /* @__PURE__ */ new Set();
  const directTables = /* @__PURE__ */ new Set();
  const nestedTables = /* @__PURE__ */ new Set();
  const projectHeaders = /* @__PURE__ */ new Set();
  const parentKeys = /* @__PURE__ */ new Set();
  let currentTable = [];
  let rootInline = null;
  let projectsTableSeen = false;
  let invalid = false;
  for (const line of lines) {
    const parts = headerParts(line);
    if (parts) {
      currentTable = parts;
      if (parts[0] === "projects") {
        if (rootInline) invalid = true;
        if (parts.length === 1) {
          if (projectsTableSeen) invalid = true;
          projectsTableSeen = true;
        } else {
          const key = normalizedPath(parts[1]);
          const headerKey = JSON.stringify([key, ...parts.slice(2)]);
          if (projectHeaders.has(headerKey)) invalid = true;
          projectHeaders.add(headerKey);
          paths.add(key);
          if (parts.length === 2) {
            if (directTables.has(key) || parentKeys.has(key)) invalid = true;
            directTables.add(key);
          } else {
            nestedTables.add(key);
          }
        }
      }
      continue;
    }
    const relevantHeader = withoutComment(line).trim();
    if (relevantHeader.startsWith("[")) {
      if (/^\[\[\s*projects(?:\s*\.|\s*\])/.test(relevantHeader) || /^\[\s*projects(?:\s*\.|\s*\])/.test(relevantHeader)) invalid = true;
      currentTable = [];
      continue;
    }
    const assignment = assignmentParts(line);
    if (!assignment) {
      if (currentTable.length === 1 && currentTable[0] === "projects" && relevantHeader.length > 0 && !relevantHeader.startsWith("#")) invalid = true;
      if (currentTable.length === 0 && /^projects\s*=/.test(relevantHeader)) invalid = true;
      continue;
    }
    if (currentTable.length === 0 && assignment.key[0] === "projects") {
      if (assignment.key.length === 1) {
        const keys = inlineTableKeys(assignment.value);
        if (!keys || rootInline) {
          invalid = true;
          continue;
        }
        rootInline = /* @__PURE__ */ new Set();
        for (const key of keys) {
          const normalized = normalizedPath(key);
          if (rootInline.has(normalized)) invalid = true;
          rootInline.add(normalized);
          paths.add(normalized);
        }
      } else {
        const key = normalizedPath(assignment.key[1]);
        if (parentKeys.has(key) || directTables.has(key)) invalid = true;
        parentKeys.add(key);
        paths.add(key);
      }
      continue;
    }
    if (currentTable.length === 1 && currentTable[0] === "projects") {
      const key = normalizedPath(assignment.key[0]);
      if (parentKeys.has(key) || directTables.has(key)) invalid = true;
      parentKeys.add(key);
      paths.add(key);
    }
  }
  for (const key of parentKeys) {
    if (nestedTables.has(key)) invalid = true;
  }
  if (rootInline && (projectsTableSeen || directTables.size > 0 || nestedTables.size > 0 || parentKeys.size > 0)) invalid = true;
  return { paths, rootInline, invalid };
}
function mergeTrustedProjectEntries(profile, config) {
  const entries = collectProfileEntries(profile);
  if (!entries) return null;
  if (entries.length === 0) return config;
  const existing = collectExistingPaths(config);
  if (existing.invalid) return null;
  const missing = entries.filter((entry) => {
    const key = normalizedPath(entry.path);
    return !existing.paths.has(key) && !existing.rootInline?.has(key);
  });
  if (existing.rootInline && missing.length) return null;
  if (missing.length === 0) return config;
  const newlineSource = config || profile;
  const newline = newlineSource.includes("\r\n") ? "\r\n" : "\n";
  const separator = !config ? "" : config.endsWith(newline + newline) ? "" : config.endsWith(newline) ? newline : newline + newline;
  const appended = missing.flatMap((entry) => [entry.header, entry.assignment]).join(newline) + newline;
  return config + separator + appended;
}
function carryCodexProjectTrust(host2, profilePath2, configPath) {
  const lockId = host2.fs.lockFile(configPath + ".project-trust.lock");
  if (lockId === null) {
    host2.log?.("warn", "Codex project trust carry-over skipped because the shared config lock is busy");
    return false;
  }
  try {
    if (host2.fs.isSymlink(configPath) !== false) {
      host2.log?.("warn", "Codex project trust carry-over skipped because the shared config path could not be safely replaced");
      return false;
    }
    const profile = host2.fs.readFile(profilePath2);
    if (profile === null) return false;
    const configExists = host2.fs.fileExists(configPath);
    const config = configExists ? host2.fs.readFile(configPath) : "";
    if (config === null) return false;
    const merged = mergeTrustedProjectEntries(profile, config);
    if (merged === null) {
      host2.log?.("warn", "Codex project trust carry-over skipped because the TOML structure is ambiguous");
      return false;
    }
    if (merged === config) return true;
    const currentExists = host2.fs.fileExists(configPath);
    const current = currentExists ? host2.fs.readFile(configPath) : "";
    if (current === null || currentExists !== configExists || current !== config) {
      host2.log?.("warn", "Codex project trust carry-over skipped after a concurrent config change");
      return false;
    }
    if (host2.fs.isSymlink(configPath) !== false) {
      host2.log?.("warn", "Codex project trust carry-over skipped because the shared config path could not be safely replaced");
      return false;
    }
    if (!host2.fs.writeFileAtomic(configPath, merged)) {
      host2.log?.("warn", "Codex project trust carry-over could not atomically update the shared config");
      return false;
    }
    return true;
  } finally {
    host2.fs.unlockFile(lockId);
  }
}

// plugins/provider-codex/src/profile-sweep.ts
var CODEX_PROFILE_NAME_RE = /^codeterm-([A-Za-z0-9_-]+)\.config\.toml$/;
var INSTANCE_MARKER_RE = /^# codeterm-instance: ([A-Za-z0-9_-]+)$/;
function codexProfileTabId(fileName) {
  return CODEX_PROFILE_NAME_RE.exec(fileName)?.[1] ?? null;
}
function codexProfileInstanceId(contents) {
  const firstLine = contents.replace(/^\uFEFF/, "").split(/\r?\n/, 1)[0];
  return INSTANCE_MARKER_RE.exec(firstLine)?.[1] ?? null;
}
function profilePath(host2, name) {
  if (host2.platform() !== "windows") return null;
  const home = host2.homeDir();
  return home ? `${home}\\.codex\\${name}` : null;
}
function profiles(host2) {
  if (host2.platform() !== "windows") return null;
  const home = host2.homeDir();
  if (!home) return null;
  const dir = `${home}\\.codex`;
  try {
    const entries = host2.fs.readDir(dir);
    if (!Array.isArray(entries)) return null;
    return entries.filter((entry) => entry?.isFile).map((entry) => entry.name);
  } catch {
    return null;
  }
}
function readProfile(host2, path) {
  try {
    return host2.fs.readFile(path);
  } catch {
    return null;
  }
}
function removeProfile(host2, path) {
  try {
    return host2.fs.removeFile(path);
  } catch {
    return false;
  }
}
function removeProfileAfterTrustCarry(host2, path) {
  const home = host2.homeDir();
  if (!home) return false;
  const configPath = `${home}\\.codex\\config.toml`;
  return carryCodexProjectTrust(host2, path, configPath) && removeProfile(host2, path);
}
function inspectCodexProfiles(host2, liveTabIds, instanceId) {
  const listing = profiles(host2);
  if (!listing) return { dead: [], legacy: [] };
  const live = new Set(liveTabIds);
  const dead = [];
  const legacy = [];
  for (const name of listing) {
    const tabId = codexProfileTabId(name);
    if (tabId === null) continue;
    const path = profilePath(host2, name);
    if (!path) continue;
    const contents = readProfile(host2, path);
    if (contents === null) continue;
    const owner = codexProfileInstanceId(contents);
    if (owner === instanceId && !live.has(tabId)) dead.push(name);
    else if (owner === null) legacy.push(name);
  }
  return { dead, legacy };
}
function sweepCodexProfiles(host2, liveTabIds, instanceId) {
  return manageCodexProfiles(host2, liveTabIds, instanceId, true, false);
}
function manageCodexProfiles(host2, liveTabIds, instanceId, confirm, includeLegacy) {
  const candidates = inspectCodexProfiles(host2, liveTabIds, instanceId);
  const removed = [];
  if (!confirm) return { ...candidates, removed };
  for (const name of [...candidates.dead, ...includeLegacy ? candidates.legacy : []]) {
    const path = profilePath(host2, name);
    if (!path) continue;
    const contents = readProfile(host2, path);
    if (contents === null) continue;
    const owner = codexProfileInstanceId(contents);
    const stillOwned = owner === instanceId;
    const stillLegacy = includeLegacy && owner === null;
    if ((stillOwned || stillLegacy) && removeProfileAfterTrustCarry(host2, path)) removed.push(name);
  }
  return { ...candidates, removed };
}
function releaseCodexProfile(host2, tabId, instanceId) {
  const suffix = tabId.replace(/[^A-Za-z0-9_-]/g, "-");
  const name = `codeterm-${suffix}.config.toml`;
  const path = profilePath(host2, name);
  if (!path || codexProfileTabId(name) === null) return false;
  const contents = readProfile(host2, path);
  if (contents === null || codexProfileInstanceId(contents) !== instanceId) return false;
  return removeProfileAfterTrustCarry(host2, path);
}

// plugins/provider-codex/src/tabState.ts
var CLASSIFY_THRESHOLD = 0.8;
var RESUME_BUFFER_MS = 60 * 1e3;
function classifyCodexTabState(snapshot, nowMs) {
  const screen = String(snapshot && snapshot.screenText || "").replace(/\r\n?/g, "\n");
  if (!screen.trim()) return null;
  if (looksBusy(screen)) return { state: "working", confidence: 0 };
  const activeChoice = parseActiveTerminalChoice(screen);
  const authConfidence = activeChoice ? scoreAuthPrompt(activeChoice.options.map((option) => option.label).join("\n")) : 0;
  if (authConfidence >= CLASSIFY_THRESHOLD) {
    return {
      state: "waiting_on_auth",
      confidence: authConfidence,
      action: { surface: { kind: "auth_prompt", payload: { reason: "login_detected" } } }
    };
  }
  if (looksLikeModelPicker(screen)) return { state: "working", confidence: 0 };
  const update = scoreUpdatePrompt(screen);
  if (update.confidence >= CLASSIFY_THRESHOLD) {
    return {
      state: "update_prompt",
      confidence: update.confidence
    };
  }
  const rateLimit = scoreRateLimit(screen, nowMs);
  if (rateLimit.confidence >= CLASSIFY_THRESHOLD) {
    const action = {};
    const data = {};
    if (rateLimit.resetAtMs !== null) {
      data.resetAt = new Date(rateLimit.resetAtMs).toISOString();
      action.delayedResume = {
        at: new Date(rateLimit.resetAtMs + RESUME_BUFFER_MS).toISOString(),
        text: "continue your work",
        thenKeys: ["enter"]
      };
    } else {
      action.surface = { kind: "rate_limit", payload: { reason: "reset_time_unparsed" } };
    }
    return {
      state: "rate_limited",
      confidence: rateLimit.confidence,
      data: rateLimit.resetAtMs !== null ? data : void 0,
      action
    };
  }
  if (activeChoice) {
    return {
      state: "clarifying_question",
      confidence: 1,
      data: { questions: [activeChoice.question] },
      action: {
        surface: {
          kind: "clarifying_question",
          payload: { questions: [activeChoice.question] }
        }
      }
    };
  }
  const partial = rateLimit.confidence > update.confidence ? rateLimit.confidence : update.confidence;
  if (partial > 0) return { state: "unknown", confidence: partial };
  return { state: "working", confidence: 0 };
}
function scoreUpdatePrompt(screen) {
  const options = parseActiveTerminalChoice(screen)?.options ?? [];
  let hasUpdateOption = false;
  let hasSkipOption = false;
  for (let i = 0; i < options.length; i++) {
    if (/\bupdate\b/i.test(options[i].label)) hasUpdateOption = true;
    if (/\bskip\b/i.test(options[i].label)) hasSkipOption = true;
  }
  const actionableMenu = hasUpdateOption && hasSkipOption;
  let confidence = 0;
  if (actionableMenu) confidence += 0.45;
  if (/\b(update|version)\s+available\b/i.test(screen)) confidence += 0.2;
  if (/\d+\.\d+(?:\.\d+)?\s*(?:->|→|=>)\s*\d+\.\d+(?:\.\d+)?/.test(screen)) {
    confidence += 0.15;
  }
  if (/press\s+enter\s+to\s+continue/i.test(screen)) confidence += 0.1;
  if (/release\s+notes/i.test(screen)) confidence += 0.1;
  if (confidence > 1) confidence = 1;
  return { confidence };
}
function looksBusy(screen) {
  return /\bctrl\+c to (?:cancel|stop|interrupt|quit)\b/i.test(screen) || /\bWorking[….]/.test(screen) || /\((?:esc|ctrl\+c) to interrupt\)/i.test(screen);
}
function looksLikeModelPicker(screen) {
  if (!/\bselect\s+(?:model|reasoning)\b/i.test(screen)) return false;
  return /\bpress\s+enter\s+to\s+confirm\b/i.test(screen) && /\besc\s+to\s+go\s+back\b/i.test(screen);
}
function scoreRateLimit(screen, nowMs) {
  let confidence = 0;
  if (/\bhit your usage limit\b/i.test(screen) || /\breached your usage limit\b/i.test(screen)) {
    confidence += 0.5;
  }
  if (/\btry again at\b/i.test(screen)) confidence += 0.3;
  if (/chatgpt\.com\/codex\/settings/i.test(screen)) confidence += 0.1;
  if (/\btry again at\b[^\n]{0,48}?\d{1,2}(?::\d{2})?\s*(?:am|pm)\b/i.test(screen)) {
    confidence += 0.1;
  }
  if (confidence > 1) confidence = 1;
  return { confidence, resetAtMs: parseTryAgainClock(screen, nowMs) };
}
function parseTryAgainClock(screen, nowMs) {
  const match = /\btry again at\b[^\n]{0,48}?(\d{1,2}(?::\d{2})?\s*(?:am|pm))\b/i.exec(screen);
  return match ? parseTodayClock(match[1], nowMs) : null;
}
function parseTodayClock(value, nowMs) {
  const match = /^(\d{1,2})(?::(\d{2}))?\s*(am|pm)$/i.exec(value.trim());
  if (!match) return null;
  const hour = Number.parseInt(match[1], 10);
  const minute = Number.parseInt(match[2] || "0", 10);
  if (hour === 0 || hour > 12 || minute > 59) return null;
  const hour24 = hour % 12 + (match[3].toLowerCase() === "pm" ? 12 : 0);
  const now = new Date(nowMs);
  let candidate = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, minute, 0, 0);
  if (candidate.getTime() <= nowMs) {
    candidate = new Date(candidate.getTime() + 24 * 3600 * 1e3);
  }
  return candidate.getTime();
}
function scoreAuthPrompt(screen) {
  let score = 0;
  if (/\bsign[ -]?in\b/i.test(screen)) score += 0.5;
  if (/\blog[ -]?in\b/i.test(screen)) score += 0.5;
  if (/\bauthenticat(?:e|ion|ing)\b/i.test(screen)) score += 0.5;
  if (/\bo\s?auth\b/i.test(screen)) score += 0.6;
  if (/\bapi key\b/i.test(screen)) score += 0.6;
  if (/paste\b[^\n]*\b(token|key|code|credential)/i.test(screen)) score += 0.6;
  if (/\bauthorization code\b/i.test(screen)) score += 0.6;
  if (/enter your\b[^\n]*\b(api key|token|credential)/i.test(screen)) score += 0.6;
  return score > 1 ? 1 : score;
}

// plugins/provider-codex/src/hooks.ts
var CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
var LEGACY_CODEX_HOOK_COMMANDS = /* @__PURE__ */ new Set([
  `curl -s -X POST http://localhost:\${CODETERM_API_PORT:-7686}/api/tabs/$CODETERM_TAB_ID/hook/session_start -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' -H "X-CodeTerm-Binding-Nonce: \${CODETERM_SESSION_BINDING_NONCE}" -H "Authorization: Bearer \${CODETERM_API_TOKEN}" --data-binary @- > /dev/null 2>&1 || true`,
  "curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' --data-binary @- > /dev/null 2>&1 || true"
]);
for (const event of ["session_start", "stop"]) {
  LEGACY_CODEX_HOOK_COMMANDS.add(
    'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/' + event + `" -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' -H "X-CodeTerm-Binding-Nonce: \${CODETERM_SESSION_BINDING_NONCE}" -H "Authorization: Bearer \${CODETERM_API_TOKEN}" --data-binary @- > /dev/null 2>&1 || true; fi`
  );
}
var CODEX_HOOK_TIMEOUT_SECS = 60;
var CODEX_HOOK_REQUEST_TIMEOUT_SECS = 30;
function codexWindowsHookScript() {
  return [
    "@echo off",
    "setlocal DisableDelayedExpansion",
    "rem " + CODETERM_CODEX_HOOK_MARKER,
    "if not defined CODETERM_TAB_ID exit /b 0",
    "if not defined CODETERM_API_URL exit /b 0",
    "if not defined CODETERM_API_TOKEN exit /b 0",
    "if not defined CODETERM_SESSION_BINDING_NONCE exit /b 0",
    'set "hookEvent=%~1"',
    'set "hookMarker=%~2 %~3"',
    'if /I not "%hookMarker%"=="X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER + '" exit /b 0',
    'if /I "%hookEvent%"=="session_start" goto post_event',
    'if /I "%hookEvent%"=="stop" goto post_event',
    "exit /b 0",
    ":post_event",
    'set "apiBase=%CODETERM_API_URL%"',
    'if "%apiBase:~-1%"=="/" set "apiBase=%apiBase:~0,-1%"',
    'set "responseFile=%TEMP%\\codeterm-hook-%CODETERM_TAB_ID%-%hookEvent%-%RANDOM%.tmp"',
    "curl.exe --silent --show-error --max-time " + CODEX_HOOK_REQUEST_TIMEOUT_SECS + ' --request POST "%apiBase%/api/tabs/%CODETERM_TAB_ID%/hook/%hookEvent%" --header "Content-Type: application/json" --header "X-CodeTerm-Agent-Provider: codex" --header "%hookMarker%" --header "X-CodeTerm-Binding-Nonce: %CODETERM_SESSION_BINDING_NONCE%" --header "Authorization: Bearer %CODETERM_API_TOKEN%" --output "%responseFile%" --data-binary @-',
    'if exist "%responseFile%" del /F /Q "%responseFile%"',
    "exit /b 0",
    ""
  ].join("\r\n");
}
function isCodetermCodexHook(entry) {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    if (isOwnedCodexHook(hooks[i])) return true;
  }
  return false;
}
function removeOwnedCodexHooks(entries) {
  const out = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    const hooks = entry && entry.hooks;
    if (!Array.isArray(hooks)) {
      out.push(entry);
      continue;
    }
    const kept = hooks.filter((hook) => !isOwnedCodexHook(hook));
    if (kept.length === hooks.length) out.push(entry);
    else if (kept.length > 0) out.push({ ...entry, hooks: kept });
  }
  return out;
}
function isOwnedCodexHook(hook) {
  if (!hook) return false;
  const command = String(hook.command || "");
  const fullCommand = command + "\n" + String(hook.commandWindows || "");
  if (fullCommand.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1 || fullCommand.indexOf("X-CodeTerm-Hook:" + CODETERM_CODEX_HOOK_MARKER) !== -1) return true;
  if (hook._source !== void 0) return hook._source === CODETERM_CODEX_HOOK_MARKER;
  return LEGACY_CODEX_HOOK_COMMANDS.has(command);
}
var CODEX_HOOK_SCRIPT_CONTRACT = [
  "rem " + CODETERM_CODEX_HOOK_MARKER,
  "hookEvent",
  "CODETERM_TAB_ID",
  "CODETERM_API_URL",
  "CODETERM_API_TOKEN",
  "CODETERM_SESSION_BINDING_NONCE",
  "X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER,
  "/api/tabs/",
  "curl.exe",
  "--max-time " + CODEX_HOOK_REQUEST_TIMEOUT_SECS,
  "responseFile",
  "--output"
];
function hasAdequateCodexHookTimeout(entry) {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks) || hooks.length === 0) return false;
  return hooks.every((hook) => {
    const timeout = hook && hook.timeout;
    return typeof timeout === "number" && Number.isFinite(timeout) && timeout >= CODEX_HOOK_TIMEOUT_SECS;
  });
}
function codexHookContentHash(text5) {
  let hash = 2166136261;
  for (let i = 0; i < text5.length; i++) {
    hash ^= text5.charCodeAt(i);
    hash = hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24)) >>> 0;
  }
  return ("0000000" + hash.toString(16)).slice(-8);
}
function isOwnedCodexHookScriptContent(text5) {
  return text5.indexOf(CODETERM_CODEX_HOOK_MARKER) !== -1;
}
function isCurrentCodexHookScriptContent(text5) {
  if (!isOwnedCodexHookScriptContent(text5)) return false;
  if (text5.indexOf("codeterm-profile-cleanup-v1") !== -1) return false;
  if (text5.indexOf("codeterm-%CODETERM_TAB_ID%.config.toml") !== -1) return false;
  if (/:\d{2,5}/.test(text5)) return false;
  if (text5.indexOf("CODETERM_API_PORT") !== -1 || /127\.0\.0\.1|localhost/i.test(text5)) return false;
  for (let i = 0; i < CODEX_HOOK_SCRIPT_CONTRACT.length; i++) {
    if (text5.indexOf(CODEX_HOOK_SCRIPT_CONTRACT[i]) === -1) return false;
  }
  return true;
}
function isOwnedCodexWindowsHookScript(path) {
  const text5 = host.fs.readFile(path);
  return typeof text5 === "string" && isOwnedCodexHookScriptContent(text5);
}

// plugins/provider-codex/src/appServer.ts
var APP_SERVER_EXIT_DEADLINE_MS = 2e4;
var JSON_RPC_METHOD_NOT_FOUND = -32601;
function stderrIndicatesMissingAppServer(stderr) {
  return /unrecognized subcommand|unknown subcommand|invalid subcommand|unexpected argument/i.test(stderr);
}
function isMethodNotFound(error) {
  return error.code === JSON_RPC_METHOD_NOT_FOUND || /method not found/i.test(String(error.message || ""));
}
function parseAppServerSession(raw) {
  let result;
  try {
    result = typeof raw === "string" ? JSON.parse(raw) : raw;
  } catch {
    return { kind: "failed", reason: "host.exec returned invalid JSON" };
  }
  if (!result || typeof result !== "object") {
    return { kind: "failed", reason: "host.exec returned invalid JSON" };
  }
  const stderr = String(result.stderr || "");
  if (typeof result.error === "string" && result.error) {
    if (result.error.indexOf("spawn ") === 0) {
      return { kind: "unavailable", reason: "codex binary unavailable: " + result.error };
    }
    return { kind: "failed", reason: result.error };
  }
  const responses = {};
  const outLines = String(result.stdout || "").split("\n");
  for (let i = 0; i < outLines.length; i++) {
    const line = outLines[i].trim();
    if (!line) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (typeof message.id === "number") responses[message.id] = { result: message.result, error: message.error };
  }
  const init = responses[1];
  if (!init) {
    if (stderrIndicatesMissingAppServer(stderr)) {
      return { kind: "unavailable", reason: "codex CLI does not support app-server: " + stderr.slice(0, 400) };
    }
    return { kind: "failed", reason: "codex app-server exited before responding" + (stderr ? ": " + stderr.slice(0, 400) : "") };
  }
  if (init.error) {
    if (isMethodNotFound(init.error)) return { kind: "unavailable", reason: "codex app-server does not support initialize" };
    return { kind: "failed", reason: "initialize failed: " + String(init.error.message || "unknown error") };
  }
  if (typeof result.code === "number" && result.code !== 0) {
    return { kind: "failed", reason: "codex app-server exited with code " + result.code };
  }
  return { kind: "ok", responses };
}
function runAppServerSession(requests, then) {
  const initialize = JSON.stringify({
    method: "initialize",
    id: 1,
    params: {
      clientInfo: { name: "codeterm", title: "CodeTerm", version: "0.0.0" },
      capabilities: { experimentalApi: true }
    }
  });
  const requestLines = [JSON.stringify({ method: "initialized" })];
  const responseIds = [];
  for (let i = 0; i < requests.length; i++) {
    const req = { method: requests[i].method, id: requests[i].id };
    if (requests[i].params !== void 0) req.params = requests[i].params;
    requestLines.push(JSON.stringify(req));
    if (typeof requests[i].id === "number") responseIds.push(requests[i].id);
  }
  const opts = {
    bin: "codex",
    args: ["app-server"],
    conversation: [
      { stdin: initialize + "\n", awaitResponseIds: [1] },
      { stdin: requestLines.join("\n") + "\n", awaitResponseIds: responseIds }
    ],
    timeoutMs: APP_SERVER_EXIT_DEADLINE_MS
  };
  try {
    const asyncExec = host.exec.async;
    return typeof asyncExec === "function" ? asyncExec(opts, (raw) => then(parseAppServerSession(raw))) : then(parseAppServerSession(host.exec(JSON.stringify(opts))));
  } catch (e) {
    return then({ kind: "failed", reason: "host.exec threw: " + String(e) });
  }
}
function requestOutcome(session, id, method) {
  if (session.kind !== "ok") return session;
  const response = session.responses[id];
  if (!response) return { kind: "failed", reason: "codex app-server returned no response for " + method };
  if (response.error) {
    if (isMethodNotFound(response.error)) {
      return { kind: "unavailable", reason: "codex app-server does not support " + method };
    }
    return { kind: "failed", reason: method + " failed: " + String(response.error.message || "unknown error") };
  }
  return { kind: "ok", result: response.result };
}
function collectHookListings(result) {
  const data = result && typeof result === "object" && Array.isArray(result.data) ? result.data : [];
  const listings = [];
  const seen = {};
  for (let i = 0; i < data.length; i++) {
    const hooks = data[i] && Array.isArray(data[i].hooks) ? data[i].hooks : [];
    for (let j = 0; j < hooks.length; j++) {
      const hook = hooks[j];
      if (!hook || typeof hook.key !== "string" || typeof hook.currentHash !== "string" || typeof hook.trustStatus !== "string") {
        continue;
      }
      if (seen[hook.key]) continue;
      seen[hook.key] = true;
      listings.push({
        key: hook.key,
        command: typeof hook.command === "string" ? hook.command : null,
        currentHash: hook.currentHash,
        trustStatus: hook.trustStatus
      });
    }
  }
  return listings;
}
function slashNormalize(value) {
  return value.split("\\").join("/");
}
function keyMatchesManagedLabel(key, hooksPath, label) {
  return slashNormalize(key).indexOf(slashNormalize(hooksPath) + ":" + label + ":") === 0;
}
function isCodetermManagedHookCommand(command) {
  const c = String(command || "");
  return c.indexOf("/hook/session_start") !== -1 || c.indexOf("/hook/stop") !== -1 || c.indexOf("codeterm-session-start.ps1") !== -1 || c.indexOf("codeterm-session-hook.cmd") !== -1 && c.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1;
}
function managedListings(listings, hooksPath, labels) {
  const out = [];
  for (let i = 0; i < listings.length; i++) {
    const listing = listings[i];
    if (!isCodetermManagedHookCommand(listing.command)) continue;
    for (let j = 0; j < labels.length; j++) {
      if (keyMatchesManagedLabel(listing.key, hooksPath, labels[j])) {
        out.push(listing);
        break;
      }
    }
  }
  return out;
}
function trustTableKey(line) {
  const value = line.trim();
  const prefix = "[hooks.state.";
  if (!value.startsWith(prefix) || !value.endsWith("]")) return null;
  const raw = value.slice(prefix.length, -1);
  if (raw.length >= 2 && raw[0] === "'" && raw[raw.length - 1] === "'") {
    return raw.slice(1, -1);
  }
  if (raw.startsWith('"') && raw.endsWith('"')) {
    try {
      return JSON.parse(raw);
    } catch (_) {
      return null;
    }
  }
  return null;
}
function recordedTrustedHash(config, key) {
  const lines = config.split("\n");
  const normalizedKey = slashNormalize(key);
  for (let i = 0; i < lines.length; i++) {
    const tableKey = trustTableKey(lines[i]);
    const normalizedTableKey = tableKey === null ? null : slashNormalize(tableKey);
    const sameKey = host.platform() === "windows" ? normalizedTableKey?.toLowerCase() === normalizedKey.toLowerCase() : normalizedTableKey === normalizedKey;
    if (!sameKey) continue;
    for (let j = i + 1; j < lines.length; j++) {
      const trimmed = lines[j].trim();
      if (trimmed.startsWith("[") && trimmed.endsWith("]")) break;
      const match = /^trusted_hash\s*=\s*(['"])([^'"\r\n]+)\1\s*(?:#.*)?\r?$/.exec(lines[j]);
      if (match) return match[2];
    }
    return null;
  }
  return null;
}
function inspectCodexHookTrust(opts, then = (inspection) => inspection) {
  return runAppServerSession([
    { method: "hooks/list", id: 2, params: { cwds: [opts.listCwd] } }
  ], (session) => {
    const result = requestOutcome(session, 2, "hooks/list");
    if (result.kind !== "ok") return then({ verdict: "unknown", reason: result.reason });
    const managed = managedListings(collectHookListings(result.result), opts.hooksPath, opts.managedLabels);
    if (managed.length !== opts.managedLabels.length) {
      return then({
        verdict: "unknown",
        reason: "Codex returned " + managed.length + " managed hooks, expected " + opts.managedLabels.length
      });
    }
    for (let i = 0; i < managed.length; i++) {
      const listing = managed[i];
      const recorded = recordedTrustedHash(opts.config, listing.key);
      if (recorded === null) {
        return then({ verdict: "unknown", reason: "No recorded trusted_hash exists for " + listing.key });
      }
      if (recorded !== listing.currentHash) {
        return then({ verdict: "stale", reason: "Recorded trusted_hash does not match Codex currentHash for " + listing.key });
      }
      if (listing.trustStatus !== "trusted") {
        return then({ verdict: "unknown", reason: "Codex reports trustStatus=" + listing.trustStatus + " for " + listing.key });
      }
    }
    return then({ verdict: "trusted" });
  });
}
function outcome(partial) {
  return {
    granted: false,
    wrote: false,
    degraded: false,
    verify_failed: false,
    ...partial
  };
}
function grantCodexHookTrust(opts, then = (result) => result) {
  if (!opts.configPath) {
    return then(outcome({ degraded: true, reason: "could not resolve Codex config path" }));
  }
  return grantCodexHookTrustFromCodex(opts, then);
}
function grantCodexHookTrustFromCodex(opts, then) {
  if (!opts.managedLabels.length) {
    return then(outcome({ degraded: true, reason: "no managed hook keys declared" }));
  }
  if (!opts.configPath) {
    return then(outcome({ degraded: true, reason: "could not resolve Codex config path" }));
  }
  return runAppServerSession([
    { method: "hooks/list", id: 2, params: { cwds: [opts.listCwd] } }
  ], (listSession) => {
    const listResult = requestOutcome(listSession, 2, "hooks/list");
    if (listResult.kind === "unavailable") return then(outcome({ degraded: true, reason: listResult.reason }));
    if (listResult.kind === "failed") return then(outcome({ reason: listResult.reason }));
    const managed = managedListings(collectHookListings(listResult.result), opts.hooksPath, opts.managedLabels);
    if (managed.length !== opts.managedLabels.length) {
      return then(outcome({
        verify_failed: true,
        reason: "hooks/list reported " + managed.length + " managed entries, expected " + opts.managedLabels.length
      }));
    }
    const needingTrust = [];
    for (let i = 0; i < managed.length; i++) {
      if (managed[i].trustStatus !== "trusted") needingTrust.push(managed[i]);
    }
    if (!needingTrust.length) {
      return then(outcome({
        granted: true,
        entries: managed.map((listing) => ({ key: listing.key, hash: listing.currentHash }))
      }));
    }
    if (!opts.configPath) return then(outcome({ reason: "Codex config path is unknown; trust was not granted" }));
    const value = {};
    for (let i = 0; i < needingTrust.length; i++) {
      const currentHash = needingTrust[i].currentHash;
      if (!currentHash) return then(outcome({ reason: "Codex reported no current hash for a hook that needs trust" }));
      value[needingTrust[i].key] = { trusted_hash: currentHash };
    }
    return runAppServerSession([
      {
        method: "config/batchWrite",
        id: 2,
        params: { edits: [{ keyPath: "hooks.state", value, mergeStrategy: "upsert" }], reloadUserConfig: true }
      },
      { method: "hooks/list", id: 3, params: { cwds: [opts.listCwd] } }
    ], (writeSession) => {
      const writeResult = requestOutcome(writeSession, 2, "config/batchWrite");
      if (writeResult.kind !== "ok") return then(outcome({ reason: writeResult.reason }));
      const writeStatus = writeResult.result && typeof writeResult.result === "object" ? writeResult.result.status : void 0;
      if (writeStatus !== "ok") {
        return then(outcome({ reason: "config/batchWrite did not confirm status=ok" }));
      }
      const verifyResult = requestOutcome(writeSession, 3, "hooks/list");
      if (verifyResult.kind !== "ok") {
        return then(outcome({ wrote: true, verify_failed: true, reason: verifyResult.reason }));
      }
      const verified = managedListings(collectHookListings(verifyResult.result), opts.hooksPath, opts.managedLabels);
      if (verified.length !== opts.managedLabels.length) {
        return then(outcome({ wrote: true, verify_failed: true, reason: "post-grant verify reported " + verified.length + " managed entries" }));
      }
      const expectedHashes = {};
      for (let i = 0; i < managed.length; i++) expectedHashes[managed[i].key] = managed[i].currentHash;
      for (let i = 0; i < verified.length; i++) {
        const listing = verified[i];
        if (listing.trustStatus !== "trusted") {
          return then(outcome({ wrote: true, verify_failed: true, reason: "post-grant verify left " + listing.key + " " + listing.trustStatus }));
        }
        if (expectedHashes[listing.key] !== listing.currentHash) {
          return then(outcome({ wrote: true, verify_failed: true, reason: "post-grant verify reported a different hash for " + listing.key }));
        }
      }
      return then(outcome({
        granted: true,
        wrote: true,
        entries: verified.map((listing) => ({ key: listing.key, hash: listing.currentHash }))
      }));
    });
  });
}

// plugins/provider-codex/src/integration.ts
var CODEX_INTEGRATION_STATE_ABSENT = "absent";
var CODEX_INTEGRATION_STATE_STALE_VERSION = "stale_version";
var CODEX_INTEGRATION_STATE_UNTRUSTED_HASH = "untrusted_hash";
var CODEX_INTEGRATION_STATE_INSTALLED = "installed";
var codexHookTrustCache = /* @__PURE__ */ new Map();
function checkIntegration() {
  const path = codexHooksPath();
  if (!path || !host.fs.fileExists(path)) {
    return emptyIntegrationStatus(path, false);
  }
  const json = host.fs.readJson(path);
  if (!json) {
    return emptyIntegrationStatus(path, true);
  }
  const arr = json.hooks && json.hooks.SessionStart;
  const stopArr = json.hooks && json.hooks.Stop;
  const installed = Array.isArray(arr) && arr.some(isCodetermCodexHook);
  const stopInstalled = Array.isArray(stopArr) && stopArr.some(isCodetermCodexHook) && (host.platform() !== "windows" || stopArr.some(hasCodetermCodexWindowsCommand));
  const winScript = codexWindowsHookScriptPath();
  const winScriptText = winScript && host.fs.fileExists(winScript) ? String(host.fs.readFile(winScript) || "") : "";
  const windowsReady = host.platform() !== "windows" || !!winScript && Array.isArray(arr) && arr.some(hasCodetermCodexWindowsCommand);
  const contractOk = host.platform() === "windows" ? isCurrentCodexHookScriptContent(winScriptText) : Array.isArray(arr) && arr.some((entry) => {
    const hk = entry && entry.hooks;
    return Array.isArray(hk) && hk.some((h) => {
      const command = String(h?.command || "");
      return command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 && command.indexOf("CODETERM_TAB_ID") !== -1 && command.indexOf("CODETERM_API_URL") !== -1 && command.indexOf("CODETERM_API_TOKEN") !== -1 && command.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1 && !hasLiteralPort(command) && command.indexOf("/api/tabs/") !== -1;
    });
  });
  const current = installed && windowsReady && contractOk && hookTimeoutsAdequate(arr, stopArr);
  const trustState = codexHookTrustState(path);
  const trustReason = trustState === "unknown" ? codexHookTrustReason(path) : void 0;
  const trustOk = trustState === "trusted";
  const integrationState = !installed ? CODEX_INTEGRATION_STATE_ABSENT : trustState === "disabled" || trustState === "stale" ? CODEX_INTEGRATION_STATE_UNTRUSTED_HASH : !current || !stopInstalled ? CODEX_INTEGRATION_STATE_STALE_VERSION : trustState === "unknown" || trustState === "enabled" ? "unknown" : !trustOk ? CODEX_INTEGRATION_STATE_UNTRUSTED_HASH : CODEX_INTEGRATION_STATE_INSTALLED;
  return {
    hooks_file_exists: true,
    hook_content_hash: ownedContentHash(arr, stopArr, winScriptText),
    hooks_installed: current && stopInstalled,
    session_start_hook: current,
    stop_hook: stopInstalled,
    hook_trust_state: trustState,
    hook_trust_reason: trustReason,
    hook_trust_enabled: trustState === "enabled" ? true : trustState === "disabled" ? false : null,
    integration_state: integrationState,
    repair_requires_consent: integrationState !== CODEX_INTEGRATION_STATE_INSTALLED,
    hooks_owned: installed,
    hook_contract_current: current && stopInstalled,
    target: path,
    backup_exists: host.fs.fileExists(path + ".codeterm-backup"),
    proposed_changes: ["SessionStart hook", "Stop hook", "hook trust"]
  };
}
function ownedContentHash(sessionStart, stop, scriptText) {
  const parts = [];
  for (const entries of [sessionStart, stop]) {
    if (!Array.isArray(entries)) continue;
    for (const entry of entries) {
      if (!isCodetermCodexHook(entry)) continue;
      const hooks = entry && entry.hooks || [];
      for (const hook of hooks) {
        parts.push(String(hook?.command || ""));
        parts.push(String(hook?.commandWindows || ""));
      }
    }
  }
  parts.push(scriptText);
  return codexHookContentHash(parts.join(String.fromCharCode(0)));
}
function emptyIntegrationStatus(path, fileExists) {
  return {
    hooks_file_exists: fileExists,
    hook_content_hash: codexHookContentHash(""),
    hooks_installed: false,
    session_start_hook: false,
    stop_hook: false,
    hook_trust_state: "missing",
    hook_trust_enabled: null,
    integration_state: CODEX_INTEGRATION_STATE_ABSENT,
    repair_requires_consent: true,
    hooks_owned: false,
    hook_contract_current: false,
    target: path || "~/.codex/hooks.json",
    backup_exists: !!path && host.fs.fileExists(path + ".codeterm-backup"),
    proposed_changes: ["SessionStart hook", "Stop hook", "hook trust"]
  };
}
function installIntegration(apiPort, options) {
  void apiPort;
  if (!options || options.consent !== true) {
    return { ok: false, error: "provider integration installation requires explicit local-user consent" };
  }
  const path = codexHooksPath();
  if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };
  const hooksExisted = host.fs.fileExists(path);
  const originalHooksValue = hooksExisted ? host.fs.readFile(path) : null;
  if (hooksExisted && typeof originalHooksValue !== "string") {
    return { ok: false, error: "could not read existing hooks JSON at " + path };
  }
  const originalHooks = typeof originalHooksValue === "string" ? originalHooksValue : "";
  let parsedHooks = {};
  if (hooksExisted) {
    try {
      parsedHooks = JSON.parse(originalHooks);
    } catch (_) {
      return { ok: false, error: "existing hooks JSON is malformed at " + path };
    }
  }
  if (!parsedHooks || typeof parsedHooks !== "object" || Array.isArray(parsedHooks)) {
    return { ok: false, error: "existing hooks JSON must contain an object at " + path };
  }
  const originalHooksJson = JSON.stringify(parsedHooks);
  const json = parsedHooks;
  if (json.hooks !== void 0 && (typeof json.hooks !== "object" || Array.isArray(json.hooks))) {
    return { ok: false, error: "hooks is not an object" };
  }
  if (json.hooks?.SessionStart !== void 0 && !Array.isArray(json.hooks.SessionStart)) {
    return { ok: false, error: "hooks.SessionStart is not an array" };
  }
  if (json.hooks?.Stop !== void 0 && !Array.isArray(json.hooks.Stop)) {
    return { ok: false, error: "hooks.Stop is not an array" };
  }
  const configPath = codexConfigPath();
  const configExisted = !!configPath && host.fs.fileExists(configPath);
  const originalConfigValue = configPath && configExisted ? host.fs.readFile(configPath) : null;
  if (configExisted && typeof originalConfigValue !== "string") {
    return { ok: false, error: "could not read existing Codex config at " + configPath };
  }
  const originalConfig = typeof originalConfigValue === "string" ? originalConfigValue : "";
  const scriptPath = host.platform() === "windows" ? codexWindowsHookScriptPath() : null;
  const scriptExisted = !!scriptPath && host.fs.fileExists(scriptPath);
  const originalScriptValue = scriptExisted && scriptPath ? host.fs.readFile(scriptPath) : null;
  if (scriptExisted && typeof originalScriptValue !== "string") {
    return { ok: false, error: "could not read existing hook script at " + scriptPath };
  }
  const originalScript = typeof originalScriptValue === "string" ? originalScriptValue : "";
  if (scriptPath && scriptExisted && !isOwnedCodexWindowsHookScript(scriptPath)) {
    return { ok: false, error: "refusing to replace unowned hook script at " + scriptPath };
  }
  const ownershipPath = codexHookTrustOwnershipPath();
  const ownershipSnapshot = ownershipPath ? captureIntegrationFile(ownershipPath) : null;
  if (ownershipPath && !ownershipSnapshot) {
    return { ok: false, error: "could not read existing Codex trust ownership record" };
  }
  const artifactPaths = [
    path + ".codeterm-backup",
    path + ".codeterm-backup-state",
    ...configPath && configExisted ? [configPath + ".codeterm-backup"] : [],
    ...scriptPath ? [scriptPath + ".codeterm-backup", scriptPath + ".codeterm-backup-state"] : []
  ];
  const artifactSnapshots = artifactPaths.map(captureIntegrationFile);
  if (artifactSnapshots.some((snapshot) => snapshot === null)) {
    return { ok: false, error: "could not snapshot existing CodeTerm integration artifacts" };
  }
  const rollbackSnapshots = [
    { path, existed: hooksExisted, bytes: originalHooks },
    ...configPath ? [{ path: configPath, existed: configExisted, bytes: originalConfig }] : [],
    ...scriptPath ? [{ path: scriptPath, existed: scriptExisted, bytes: originalScript }] : [],
    ...ownershipSnapshot ? [ownershipSnapshot] : [],
    ...artifactSnapshots
  ];
  const fail = (error) => {
    const restored = restoreIntegrationFiles(rollbackSnapshots);
    invalidateCodexHookTrust(path);
    return { ok: false, error: restored ? error : error + "; exact rollback failed" };
  };
  if (!writeFileIfChanged(path + ".codeterm-backup", originalHooks) || !writeFileIfChanged(path + ".codeterm-backup-state", hooksExisted ? "present" : "absent")) {
    return fail("could not write recoverable backup for " + path);
  }
  if (configPath && configExisted && !writeFileIfChanged(configPath + ".codeterm-backup", originalConfig)) {
    return fail("could not write recoverable backup for " + configPath);
  }
  if (scriptPath && (!writeFileIfChanged(scriptPath + ".codeterm-backup", originalScript) || !writeFileIfChanged(scriptPath + ".codeterm-backup-state", scriptExisted ? "present" : "absent"))) {
    return fail("could not write recoverable backup for " + scriptPath);
  }
  if (!json.hooks) json.hooks = {};
  for (const event of ["SessionStart", "Stop"]) {
    const existing = json.hooks[event];
    if (existing !== void 0 && !Array.isArray(existing)) {
      return { ok: false, error: "hooks." + event + " is not an array" };
    }
    json.hooks[event] = removeOwnedCodexHooks(existing || []);
  }
  const authH = '-H "Authorization: Bearer ${CODETERM_API_TOKEN}"';
  const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
  const markerH = '-H "X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER + '"';
  const guard = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ] && [ -n "${CODETERM_SESSION_BINDING_NONCE:-}" ]; then ';
  const hookCmd = (route) => guard + 'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/' + route + `" -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' ` + markerH + " " + nonceH + " " + authH + " --data-binary @- > /dev/null 2>&1 || true; fi";
  const winCmd = (route) => scriptPath ? [
    "cmd.exe /d /s /c call",
    '"' + scriptPath + '"',
    route,
    "X-CodeTerm-Hook:",
    CODETERM_CODEX_HOOK_MARKER
  ].join(" ") : hookCmd(route);
  const scriptContents = codexWindowsHookScript();
  const scriptChanged = !!scriptPath && (!scriptExisted || originalScript !== scriptContents);
  if (scriptChanged && scriptPath && !host.fs.writeFile(scriptPath, scriptContents)) {
    return fail("writeFile failed for " + scriptPath);
  }
  json.hooks.SessionStart.push({
    matcher: "startup|resume|clear|compact",
    hooks: [{
      type: "command",
      command: hookCmd("session_start"),
      commandWindows: winCmd("session_start"),
      timeout: CODEX_HOOK_TIMEOUT_SECS,
      _source: CODETERM_CODEX_HOOK_MARKER
    }]
  });
  json.hooks.Stop.push({
    hooks: [{
      type: "command",
      command: hookCmd("stop"),
      commandWindows: winCmd("stop"),
      timeout: CODEX_HOOK_TIMEOUT_SECS,
      _source: CODETERM_CODEX_HOOK_MARKER
    }]
  });
  const hooksChanged = !hooksExisted || JSON.stringify(json) !== originalHooksJson;
  const ok = hooksChanged ? host.fs.writeJson(path, json) : true;
  if (ok) {
    const trust = enableExistingCodexHookTrust(path);
    if (trust.write_failed) return fail("could not enable existing Codex hook trust");
    if (trust.changed) {
      if (!ownershipPath || !writeFileIfChanged(ownershipPath, path)) {
        return fail("could not record owned Codex hook trust expansion");
      }
    }
    if (scriptChanged || hooksChanged || trust.changed) invalidateCodexHookTrust(path);
    host.log("info", "[codex-plugin] SessionStart + Stop hooks installed at " + path);
    return { ok: true, hook_trust_state: trust.state };
  }
  return fail("writeJson failed for " + path);
}
function grantIntegrationTrust(managedKeys, then = (result) => result) {
  const hooksPath = codexHooksPath();
  if (!hooksPath) {
    return then({ granted: false, wrote: false, degraded: true, verify_failed: false, reason: "could not resolve ~/.codex/hooks.json" });
  }
  invalidateCodexHookTrust(hooksPath);
  return grantCodexHookTrust({
    hooksPath,
    configPath: codexConfigPath(),
    managedLabels: Array.isArray(managedKeys) ? managedKeys.map(String) : [],
    listCwd: host.homeDir() || "."
  }, (result) => {
    if (result.granted) cacheCodexHookTrust(hooksPath, "trusted");
    else cacheCodexHookTrust(hooksPath, "unknown", result.reason);
    return then(result);
  });
}
function uninstallIntegration() {
  const path = codexHooksPath();
  if (!path) return { ok: true };
  let hooksOk = true;
  if (host.fs.fileExists(path)) {
    const json = host.fs.readJson(path);
    if (!json || !json.hooks || typeof json.hooks !== "object") {
      return { ok: false, error: "could not safely read hooks JSON at " + path };
    }
    const before = JSON.stringify(json);
    if (Array.isArray(json.hooks.SessionStart)) {
      json.hooks.SessionStart = removeOwnedCodexHooks(json.hooks.SessionStart);
    }
    if (Array.isArray(json.hooks.Stop)) {
      json.hooks.Stop = removeOwnedCodexHooks(json.hooks.Stop);
    }
    hooksOk = before === JSON.stringify(json) || host.fs.writeJson(path, json);
  }
  const ownershipPath = codexHookTrustOwnershipPath();
  if (ownershipPath && host.fs.fileExists(ownershipPath)) {
    const ownedTarget = host.fs.readFile(ownershipPath);
    if (ownedTarget === path && disableOwnedCodexHookTrust(path)) {
      host.fs.removeFile(ownershipPath);
    }
  }
  const scriptPath = codexWindowsHookScriptPath();
  if (scriptPath && host.fs.fileExists(scriptPath) && isOwnedCodexWindowsHookScript(scriptPath)) {
    host.fs.removeFile(scriptPath);
  }
  invalidateCodexHookTrust(path);
  return hooksOk ? { ok: true } : { ok: false, error: "writeJson failed for " + path };
}
function restoreIntegration() {
  const path = codexHooksPath();
  if (!path) return { ok: false, error: "could not resolve ~/.codex/hooks.json" };
  const backup = path + ".codeterm-backup";
  if (!host.fs.fileExists(backup)) return { ok: false, error: "no recoverable backup at " + backup };
  const hooks = host.fs.readFile(backup);
  if (typeof hooks !== "string") return { ok: false, error: "restore failed for " + path };
  const hooksState = host.fs.readFile(path + ".codeterm-backup-state");
  if (hooksState === "absent" || typeof hooksState !== "string" && hooks === "") {
    if (host.fs.fileExists(path) && !host.fs.removeFile(path)) return { ok: false, error: "restore failed for " + path };
  } else if (!writeFileIfChanged(path, hooks)) {
    return { ok: false, error: "restore failed for " + path };
  }
  const config = codexConfigPath();
  if (config && host.fs.fileExists(config + ".codeterm-backup")) {
    const original = host.fs.readFile(config + ".codeterm-backup");
    if (typeof original !== "string" || !writeFileIfChanged(config, original)) return { ok: false, error: "restore failed for " + config };
    const ownershipPath = codexHookTrustOwnershipPath();
    if (ownershipPath && host.fs.fileExists(ownershipPath)) host.fs.removeFile(ownershipPath);
  }
  const script = host.platform() === "windows" ? codexWindowsHookScriptPath() : null;
  if (script && host.fs.fileExists(script + ".codeterm-backup")) {
    const original = host.fs.readFile(script + ".codeterm-backup");
    if (typeof original !== "string") return { ok: false, error: "restore failed for " + script };
    const scriptState = host.fs.readFile(script + ".codeterm-backup-state");
    if (scriptState === "absent" || typeof scriptState !== "string" && original === "") {
      if (host.fs.fileExists(script) && !host.fs.removeFile(script)) return { ok: false, error: "restore failed for " + script };
    } else if (!writeFileIfChanged(script, original)) {
      return { ok: false, error: "restore failed for " + script };
    }
  }
  invalidateCodexHookTrust(path);
  return { ok: true };
}
function ensureIntegrationOnStartup(apiPort, install, then = (result) => result) {
  void apiPort;
  void install;
  const path = codexHooksPath();
  if (!path || !host.fs.fileExists(path)) return then({ ok: true, changed: false });
  const json = host.fs.readJson(path);
  if (!json || !json.hooks) return then({ ok: true, changed: false });
  const owned = (json.hooks.SessionStart || []).filter(isCodetermCodexHook);
  if (owned.length === 0) return then({ ok: true, changed: false });
  return refreshCodexHookTrustState(path, (trustState) => {
    if (trustState === "disabled") {
      return then({ ok: true, changed: false, reason: "Codex hook trust is disabled" });
    }
    if (trustState === "unknown") {
      return then({
        ok: false,
        changed: false,
        actionable: true,
        error: "Codex could not confirm hook trust: " + codexHookTrustReason(path)
      });
    }
    const scriptPath = codexWindowsHookScriptPath();
    const scriptStale = host.platform() === "windows" && !!scriptPath && (!host.fs.fileExists(scriptPath) || !isCurrentCodexHookScriptContent(String(host.fs.readFile(scriptPath) || "")));
    const commandStale = owned.some((entry) => {
      const hooks = entry && entry.hooks || [];
      return hooks.some(
        (hook) => {
          const command = String(hook && hook.command || "");
          return command.indexOf("$CODETERM_API_URL") === -1 || command.indexOf("${CODETERM_API_TOKEN}") === -1 || command.indexOf("$CODETERM_TAB_ID") === -1 || command.indexOf("${CODETERM_SESSION_BINDING_NONCE}") === -1 || command.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) === -1 || hasLiteralPort(command) || host.platform() === "windows" && !hasCodetermCodexWindowsCommand(entry);
        }
      );
    });
    const stopEntries = json.hooks?.Stop || [];
    const ownedStopEntries = stopEntries.filter(isCodetermCodexHook);
    const stopOwned = ownedStopEntries.length > 0;
    const stopCommandStale = host.platform() === "windows" && ownedStopEntries.some((entry) => !hasCodetermCodexWindowsCommand(entry));
    const timeoutStale = owned.some((entry) => !hasAdequateCodexHookTimeout(entry));
    const trustStale = trustState === "stale";
    if (!scriptStale && !commandStale && !timeoutStale && !stopCommandStale && stopOwned && !trustStale) {
      return then({ ok: true, changed: false });
    }
    return then({
      ok: false,
      changed: false,
      actionable: true,
      error: "Codex hook repair requires recorded integration consent; open provider integration settings and approve Repair"
    });
  });
}
function codexHooksPath() {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "hooks.json";
}
function hasLiteralPort(value) {
  return /:\d{2,5}/.test(value);
}
function hookTimeoutsAdequate(sessionStart, stop) {
  const owned = [...sessionStart || [], ...stop || []].filter(isCodetermCodexHook);
  return owned.length > 0 && owned.every(hasAdequateCodexHookTimeout);
}
function codexConfigPath() {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "config.toml";
}
function codexWindowsHookScriptPath() {
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "codeterm-session-hook.cmd";
}
function codexHookTrustOwnershipPath() {
  const config = codexConfigPath();
  return config ? config + ".codeterm-hook-trust-owned" : null;
}
function captureIntegrationFile(path) {
  const existed = host.fs.fileExists(path);
  const bytes = existed ? host.fs.readFile(path) : "";
  return typeof bytes === "string" ? { path, existed, bytes } : null;
}
function writeFileIfChanged(path, contents) {
  return host.fs.readFile(path) === contents || host.fs.writeFile(path, contents);
}
function restoreIntegrationFiles(snapshots) {
  let restored = true;
  for (let i = snapshots.length - 1; i >= 0; i--) {
    const snapshot = snapshots[i];
    const ok = snapshot.existed ? writeFileIfChanged(snapshot.path, snapshot.bytes) : !host.fs.fileExists(snapshot.path) || host.fs.removeFile(snapshot.path);
    restored = ok && restored;
  }
  return restored;
}
function codexHookTrustContext(hooksPath) {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return null;
  const text5 = String(host.fs.readFile(cfg) || "");
  const block = findCodexHookTrustBlock(text5, hooksPath);
  if (block == null) return null;
  const hooks = host.fs.readJson(hooksPath)?.hooks;
  const labels = [];
  if ((hooks?.SessionStart || []).some(isCodetermCodexHook)) labels.push("session_start");
  if ((hooks?.Stop || []).some(isCodetermCodexHook)) labels.push("stop");
  const script = codexWindowsHookScriptPath();
  const scriptText = script && host.fs.fileExists(script) ? String(host.fs.readFile(script) || "") : "";
  const trustFingerprint = codexHookContentHash(hookTrustConfigSections(text5));
  const hookFingerprint = codexHookContentHash([
    ownedContentHash(hooks?.SessionStart, hooks?.Stop, scriptText),
    labels.join(",")
  ].join(String.fromCharCode(0)));
  const fingerprint = codexHookContentHash([trustFingerprint, hookFingerprint].join(String.fromCharCode(0)));
  return { config: text5, block, labels, fingerprint, hookFingerprint, trustFingerprint };
}
function hookTrustConfigSections(config) {
  const sections = [];
  let active = false;
  let lines = [];
  for (const line of config.split(/\r?\n/)) {
    const value = line.trim();
    const header = value.startsWith("[") && value.endsWith("]");
    if (header) {
      if (active) sections.push(lines.join("\n"));
      active = /^\[hooks\.state(?:\.|\])/.test(value);
      lines = active ? [line] : [];
    } else if (active) {
      lines.push(line);
    }
  }
  if (active) sections.push(lines.join("\n"));
  return sections.join(String.fromCharCode(0));
}
function cacheCodexHookTrust(hooksPath, verdict, reason) {
  const context = codexHookTrustContext(hooksPath);
  if (context) codexHookTrustCache.set(hooksPath, {
    fingerprint: context.fingerprint,
    hookFingerprint: context.hookFingerprint,
    trustFingerprint: context.trustFingerprint,
    verdict,
    reason
  });
}
function codexHookTrustState(hooksPath) {
  const context = codexHookTrustContext(hooksPath);
  if (!context) return "missing";
  const { block, fingerprint } = context;
  const m = block.match(/(?:^|\n)\s*enabled\s*=\s*(true|false)\s*(?:\n|$)/);
  if (m && m[1] === "false") return "disabled";
  if (/(?:^|\n)\s*trusted_hash\s*=/.test(block)) {
    if (!context.labels.includes("session_start")) return "unknown";
    const cached = codexHookTrustCache.get(hooksPath);
    return cached?.fingerprint === fingerprint ? cached.verdict : "unknown";
  }
  if (m) return "unknown";
  return "missing";
}
function codexHookTrustReason(hooksPath) {
  const context = codexHookTrustContext(hooksPath);
  if (!context) return "Codex hook trust configuration is unavailable";
  const cached = codexHookTrustCache.get(hooksPath);
  if (cached?.fingerprint === context.fingerprint && cached.reason) return cached.reason;
  return "Codex has not confirmed the current hook hash";
}
function invalidateCodexHookTrust(hooksPath) {
  codexHookTrustCache.delete(hooksPath);
}
function refreshCodexHookTrustState(hooksPath, then = (verdict) => verdict) {
  const context = codexHookTrustContext(hooksPath);
  if (!context) return then("missing");
  if (/(?:^|\n)\s*enabled\s*=\s*false\s*(?:\n|$)/.test(context.block)) return then("disabled");
  if (!/(?:^|\n)\s*trusted_hash\s*=/.test(context.block)) return then(codexHookTrustState(hooksPath));
  const cached = codexHookTrustCache.get(hooksPath);
  if (cached?.fingerprint === context.fingerprint) return then(cached.verdict);
  if (!context.labels.includes("session_start")) return then("unknown");
  return inspectCodexHookTrust({
    hooksPath,
    config: context.config,
    managedLabels: context.labels,
    listCwd: host.homeDir() || "."
  }, (inspected) => {
    codexHookTrustCache.set(hooksPath, {
      fingerprint: context.fingerprint,
      hookFingerprint: context.hookFingerprint,
      trustFingerprint: context.trustFingerprint,
      verdict: inspected.verdict,
      reason: inspected.reason
    });
    return then(inspected.verdict);
  });
}
function enableExistingCodexHookTrust(hooksPath) {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return { state: "missing", changed: false, write_failed: false };
  const text5 = String(host.fs.readFile(cfg) || "");
  if (!text5) return { state: "missing", changed: false, write_failed: false };
  const span = findCodexHookTrustBlockSpan(text5, hooksPath);
  if (!span) return { state: "missing", changed: false, write_failed: false };
  const block = text5.substring(span.start, span.end);
  if (!/(?:^|\n)\s*enabled\s*=\s*false\s*(?:\n|$)/.test(block)) {
    const current = codexHookTrustState(hooksPath);
    const state = current === "enabled" || current === "trusted" ? current : "unknown";
    return { state, changed: false, write_failed: false };
  }
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)false(\s*(?:\n|$))/, "$1$2true$3");
  const next = text5.substring(0, span.start) + nextBlock + text5.substring(span.end);
  return host.fs.writeFile(cfg, next) ? { state: "enabled", changed: true, write_failed: false } : { state: "unknown", changed: false, write_failed: true };
}
function disableOwnedCodexHookTrust(hooksPath) {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return true;
  const text5 = host.fs.readFile(cfg);
  if (typeof text5 !== "string") return false;
  const span = findCodexHookTrustBlockSpan(text5, hooksPath);
  if (!span) return true;
  const block = text5.substring(span.start, span.end);
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)true(\s*(?:\n|$))/, "$1$2false$3");
  if (nextBlock === block) return true;
  const next = text5.substring(0, span.start) + nextBlock + text5.substring(span.end);
  return host.fs.writeFile(cfg, next);
}
function findCodexHookTrustBlock(text5, hooksPath) {
  const span = findCodexHookTrustBlockSpan(text5, hooksPath);
  return span ? text5.substring(span.start, span.end) : null;
}
function findCodexHookTrustBlockSpan(text5, hooksPath) {
  const lines = text5.split("\n");
  const marker = normalizeCodexHookKey(hooksPath + ":session_start:");
  let offset = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    const rawKey = hookTrustKeyFromHeader(trimmed);
    if (rawKey !== null && normalizeCodexHookKey(rawKey).startsWith(marker)) {
      const start = offset;
      let end = text5.length;
      let scanOffset = offset + line.length + 1;
      for (let j = i + 1; j < lines.length; j++) {
        const t = lines[j].trim();
        if (t.charAt(0) === "[" && t.charAt(t.length - 1) === "]") {
          end = scanOffset;
          break;
        }
        scanOffset += lines[j].length + 1;
      }
      return { start, end };
    }
    offset += line.length + 1;
  }
  return null;
}
function hookTrustKeyFromHeader(header) {
  const prefix = "[hooks.state.";
  if (!header.startsWith(prefix) || !header.endsWith("]")) return null;
  const raw = header.slice(prefix.length, -1).trim();
  if (raw.startsWith("'") && raw.endsWith("'")) return raw.slice(1, -1);
  if (raw.startsWith('"') && raw.endsWith('"')) {
    try {
      return JSON.parse(raw);
    } catch {
      return raw.slice(1, -1).replace(/\\\\/g, "\\").replace(/\\\"/g, '"');
    }
  }
  return null;
}
function normalizeCodexHookKey(value) {
  const normalized = value.replace(/\\/g, "/");
  return host.platform() === "windows" ? normalized.toLowerCase() : normalized;
}
function hasCodetermCodexWindowsCommand(entry) {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const hook = hooks[i];
    if (!hook) continue;
    const command = String(hook.commandWindows || "");
    const fullCommand = String(hook.command || "") + "\n" + command;
    const owned = fullCommand.indexOf("X-CodeTerm-Hook: " + CODETERM_CODEX_HOOK_MARKER) !== -1 || fullCommand.indexOf("X-CodeTerm-Hook:" + CODETERM_CODEX_HOOK_MARKER) !== -1 || hook._source === CODETERM_CODEX_HOOK_MARKER;
    if (owned && command.startsWith('cmd.exe /d /s /c call "') && command.indexOf("codeterm-session-hook.cmd") !== -1) {
      return true;
    }
  }
  return false;
}

// plugins/provider-codex/src/plugin.ts
function profileSweepHost() {
  return {
    platform: () => host.platform(),
    homeDir: () => host.homeDir(),
    log: (level, message) => host.log(level, message),
    fs: {
      fileExists: (path) => host.fs.fileExists(path),
      isSymlink: (path) => host.fs.isSymlink(path),
      readDir: (path) => host.fs.readDir(path),
      readFile: (path) => host.fs.readFile(path),
      removeFile: (path) => host.fs.removeFile(path),
      writeFileAtomic: (path, contents) => host.fs.writeFileAtomic(path, contents),
      lockFile: (path) => host.fs.lockFile(path),
      unlockFile: (id) => host.fs.unlockFile(id)
    }
  };
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
  "codex>"
];
var TUI_FRAGMENTS = ["openai codex", "ctrl+c to cancel"];
var ASSIGNED_TASK_START_PROMPT = "Execute the assigned task. Follow the provided developer instructions.";
var IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";
function starterPrompt(params) {
  if (params.starterPromptText) return params.starterPromptText;
  if (params.starterPrompt === "no_starter") return void 0;
  if (params.starterPrompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (params.starterPrompt === "team_bootstrap") throw new Error("team_bootstrap requires host starterPromptText");
  return ASSIGNED_TASK_START_PROMPT;
}
function hasPromptAndStatusFooter(screen) {
  if (screen.indexOf("\u203A") === -1) return false;
  return screen.split(/\r?\n/).some((line) => {
    const separator = line.indexOf(" \xB7 ");
    if (separator === -1) return false;
    return /(?:[a-z]:\\|\/)/i.test(line.slice(separator + 3));
  });
}
var plugin = {
  starterPromptText(prompt) {
    return prompt === "team_bootstrap" ? void 0 : starterPrompt({ starterPrompt: prompt });
  },
  detectFromTitle(title) {
    return TITLE_RE.test(String(title || ""));
  },
  detectFromOutput(text5) {
    const t = String(text5 || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },
  screenHasTui(screen) {
    const text5 = String(screen || "");
    const lower = text5.toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return hasPromptAndStatusFooter(text5);
  },
  isOutputNoise(text5) {
    const trimmed = String(text5 || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < trimmed.length; i++) {
      const c = trimmed[i];
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  hasEventMarkers(chunk) {
    const c = String(chunk || "");
    return c.indexOf("Would you like") !== -1 || c.indexOf("approve") !== -1 || c.indexOf("run the following") !== -1 || c.indexOf("make the following") !== -1 || c.indexOf("grant these permissions") !== -1 || // screen off the auto-approve path; the universal numbered parser then extracts.
    c.indexOf("to confirm") !== -1 || c.indexOf("esc to go back") !== -1 || c.indexOf("Select Model") !== -1 || c.indexOf("Select Reasoning") !== -1 || c.indexOf("\u203A") !== -1 || c.indexOf("\u276F") !== -1;
  },
  detectEvents(text5) {
    const t = String(text5 || "");
    const events = [];
    if (t.indexOf("Would you like to run the following command") !== -1 || t.indexOf("Would you like to make the following edits") !== -1 || t.indexOf("Do you want to approve network access") !== -1 || t.indexOf("Would you like to grant these permissions") !== -1) {
      events.push("permission_request");
    }
    return events;
  },
  parsePrompt(text5) {
    return parseCodexPrompt(String(text5 || ""));
  },
  parseDirectoryTrustPrompt(text5) {
    return parseCodexFolderTrustPrompt(String(text5 || ""));
  },
  buildPromptResponse(optionIndex, screen) {
    return responseKeysForIndex(optionIndex, String(screen || ""));
  },
  directoryTrustPromptResponse(screen) {
    const view = String(screen || "");
    const prompt = classifyCodexFolderTrustPrompt(view);
    if (!prompt || prompt.kind !== "active") return null;
    if (prompt.affirmativeIndex !== 0) return [];
    if (!prompt.affirmativeSelected && responseKeysForIndex(prompt.affirmativeIndex, view)[0] === "DQ==") {
      return [];
    }
    return responseKeysForIndex(prompt.affirmativeIndex, view).slice(0, 1);
  },
  // Codex CLI has no model-list subcommand; its local cache is the only sync account-scoped source. Missing => seed.
  discoverModels() {
    return discoverCodexModels();
  },
  releaseTabLaunchArtifacts(tabId, instanceId) {
    return releaseCodexProfile(profileSweepHost(), tabId, instanceId);
  },
  sweepLaunchArtifacts(request) {
    return manageCodexProfiles(
      profileSweepHost(),
      request.liveTabIds,
      request.instanceId,
      request.confirm,
      request.includeLegacy
    );
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = [];
    let prefix = "";
    parts.push("codex");
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
      parts.push("--dangerously-bypass-hook-trust");
    }
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    if (p.task?.trim()) {
      const instanceId = typeof p.instanceId === "string" && /^[A-Za-z0-9_-]+$/.test(p.instanceId) ? p.instanceId : null;
      if (host.platform() === "windows" && !instanceId) {
        return "Write-Error 'CodeTerm refused the Codex launch: instance identity is missing, so a per-tab profile cannot be safely owned'; exit 1";
      }
      const instructions = withLaunchMarker(p.task, typeof p.launchMarker === "string" ? p.launchMarker : void 0);
      const taskPath = host.fs.writeTempFile(instructions, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write Codex task instructions");
      const quotedPath = host.shell.quoteFor(taskPath, host.platform());
      if (host.platform() === "windows") {
        const quotedInstanceId = host.shell.quoteFor(instanceId, host.platform());
        prefix = `$ctTabId = $env:CODETERM_TAB_ID; if (-not $ctTabId) { throw 'CODETERM_TAB_ID is required for a per-tab Codex profile' }; $ctTaskInstructions = [System.IO.File]::ReadAllText(${quotedPath}, [System.Text.Encoding]::UTF8); $ctTaskJson = ($ctTaskInstructions | ConvertTo-Json -Compress); $ctProfileSuffix = $ctTabId -replace '[^A-Za-z0-9_-]', '-'; $ctProfileName = 'codeterm-' + $ctProfileSuffix; $ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); if (Test-Path -LiteralPath $ctProfilePath) { $ctPriorReleased = $false; try { if ($env:CODETERM_API_URL -and $env:CODETERM_API_TOKEN) { $ctPriorReleaseUri = $env:CODETERM_API_URL.TrimEnd('/') + '/api/tabs/' + [uri]::EscapeDataString($ctTabId) + '/launch-artifacts/release'; $ctPriorRelease = Invoke-RestMethod -Method Post -Uri $ctPriorReleaseUri -Headers @{ Authorization = ('Bearer ' + $env:CODETERM_API_TOKEN) } -ContentType 'application/json' -Body '{}' -ErrorAction Stop; $ctPriorReleased = [bool]$ctPriorRelease.released } } catch { Write-Warning 'Codex project trust carry-over was not confirmed'; }; if (-not $ctPriorReleased) { $ctPreservedProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '-preserved-' + $PID + '-' + [DateTime]::UtcNow.Ticks + '.config.toml'); try { [System.IO.File]::Move($ctProfilePath, $ctPreservedProfilePath); Write-Warning 'Unreleased Codex profile was preserved'; } catch { $ctProfileName = $ctProfileName + '-retry-' + $PID + '-' + [DateTime]::UtcNow.Ticks; $ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); Write-Warning 'Unreleased Codex profile was retained; using a separate profile for this launch'; } } }; $ctInstanceId = ${quotedInstanceId}; Set-Content -LiteralPath $ctProfilePath -Encoding UTF8 -Value ('# codeterm-instance: ' + $ctInstanceId + [Environment]::NewLine + 'developer_instructions = ' + $ctTaskJson); $ctProfileReleased = $false; try { try { `;
        parts.push("--profile");
        parts.push("$ctProfileName");
      } else {
        parts.push("-c");
        parts.push(`"developer_instructions=$(cat ${quotedPath})"`);
      }
      const prompt = starterPrompt(p);
      if (prompt) {
        parts.push("--");
        parts.push(host.shell.quoteFor(prompt, host.platform()));
      }
      if (host.platform() === "windows") {
        return prefix + parts.join(" ") + " } finally { if (Test-Path -LiteralPath $ctProfilePath) { $ctProfileReleased = $false; try { if ($env:CODETERM_API_URL -and $env:CODETERM_API_TOKEN) { $ctReleaseUri = $env:CODETERM_API_URL.TrimEnd('/') + '/api/tabs/' + [uri]::EscapeDataString($ctTabId) + '/launch-artifacts/release'; $ctRelease = Invoke-RestMethod -Method Post -Uri $ctReleaseUri -Headers @{ Authorization = ('Bearer ' + $env:CODETERM_API_TOKEN) } -ContentType 'application/json' -Body '{}' -ErrorAction Stop; $ctProfileReleased = [bool]$ctRelease.released } } catch { Write-Warning 'Codex profile retained because project trust could not be persisted'; }; } } } finally { if ($ctProfileReleased -and (Test-Path -LiteralPath $ctProfilePath)) { Remove-Item -LiteralPath $ctProfilePath -Force -ErrorAction SilentlyContinue } }";
      }
    }
    return prefix + parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const parts = ["codex", "resume"];
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
      parts.push("--dangerously-bypass-hook-trust");
    }
    const uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quoteFor(uuid, host.platform()));
    return parts.join(" ");
  },
  sessionFilePath(_cwd, sessionId) {
    return sessionFilePath(_cwd, sessionId);
  },
  sessionExists(cwd, sessionId) {
    return sessionExists(cwd, sessionId);
  },
  detectLaunchSession(evidence) {
    return detectCodexLaunchSession(evidence);
  },
  // Codex 0.153 has no per-session JSONL transcript; keep the path capability honest.
  sessionJsonlPath(cwd, sessionId) {
    return this.sessionFilePath(cwd, sessionId);
  },
  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts.
  parseSessionDelta(chunk, context) {
    const out = [];
    const commandPairing = commandPairingState(context);
    const activityIds = /* @__PURE__ */ new Set();
    const pushOnce = (row2) => {
      if (!row2 || row2.uuid && activityIds.has(row2.uuid)) return;
      if (row2.uuid) activityIds.add(row2.uuid);
      out.push(row2);
    };
    const lines = String(chunk || "").split("\n");
    let stamped = 0;
    const stampRows = (lineIndex) => {
      for (; stamped < out.length; stamped++) {
        out[stamped].recordIndex = lineIndex;
      }
    };
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      stampRows(i - 1);
      if (!lines[i]) continue;
      let v;
      try {
        v = JSON.parse(lines[i]);
      } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!validateEntry(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      let ts = null;
      const tsIso = typeof v.timestamp === "string" ? v.timestamp : null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      const normalizedEventType = v.payload?.type;
      if (normalizedEventType === "context_compacted") {
        out.push({
          role: "system",
          blocks: [{ kind: "text", data: { text: "context compacted" } }],
          ts,
          timestamp: tsIso,
          timelineMarker: { kind: "context_compaction" }
        });
        continue;
      }
      if (normalizedEventType === "thread_goal_updated") {
        const sessionIntent = sessionIntentFromPayload(v.payload);
        if (!sessionIntent) continue;
        out.push({
          role: "custom",
          blocks: [],
          ts,
          timestamp: tsIso,
          sessionIntent
        });
        continue;
      }
      if (v.type === "event_msg" && (v.payload?.type === "task_started" || v.payload?.type === "task_complete" || v.payload?.type === "turn_aborted")) {
        const turnId = typeof v.payload.turn_id === "string" ? v.payload.turn_id : "";
        if (!turnId) continue;
        clearTurnPairings(commandPairing, turnId);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_flow", data: { state: v.payload.type === "task_started" ? "start" : "end", flow_id: turnId } }],
          ts,
          timestamp: tsIso
        });
        continue;
      }
      if (v.type === "event_msg" && v.payload?.type === "item_completed") {
        if (pairCommandExecution(v.payload.item, v.payload.turn_id, commandPairing)) continue;
        pushOnce(completedActivity(v.payload.item, ts, tsIso, v.payload.turn_id));
        continue;
      }
      if (v.type === "event_msg" && v.payload?.type === "sub_agent_activity") {
        pushOnce(subAgentRow(v.payload, v.payload.event_id, void 0, ts, tsIso));
        continue;
      }
      if (v.type === "event_msg" && (v.payload?.type === "web_search_end" || v.payload?.type === "image_generation_end")) {
        const result = v.payload.type === "web_search_end" ? webSearchEndResult(v.payload, ts, tsIso) : imageGenerationEndResult(v.payload, ts, tsIso);
        if (result && !activityIds.has(result.uuid)) {
          activityIds.add(result.uuid);
          out.push(result);
        }
        continue;
      }
      if (v.type === "event_msg" && v.payload && v.payload.type === "patch_apply_end") {
        const p2 = v.payload;
        const content = typeof p2.stdout === "string" ? p2.stdout : "";
        const callId = typeof p2.call_id === "string" ? p2.call_id : "";
        const diffs = codexFileDiffs(p2.changes);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content, tool_use_id: callId, ...diffs.length > 0 ? { file_diffs: diffs } : {} } }],
          ts,
          timestamp: tsIso
        });
        continue;
      }
      if (!v || v.type !== "response_item" || !v.payload) continue;
      const p = v.payload;
      if (p.type === "message") {
        const role = p.role === "assistant" ? "assistant" : p.role === "user" ? "user" : null;
        if (!role) continue;
        let text5 = "";
        if (Array.isArray(p.content)) {
          for (let j = 0; j < p.content.length; j++) {
            const c = p.content[j];
            if (c && typeof c.text === "string") text5 += c.text;
          }
        } else if (typeof p.content === "string") {
          text5 = p.content;
        }
        if (!text5) continue;
        const systemSubtype = classifyCodexSystemText(text5);
        if (systemSubtype) {
          out.push({
            role: "system",
            blocks: [{ kind: "text", data: { text: text5, subtype: systemSubtype } }],
            sourceType: v.type,
            sourceSubtype: systemSubtype,
            ts,
            timestamp: tsIso
          });
          continue;
        }
        out.push({ role, blocks: [{ kind: "text", data: { text: text5 } }], ts, timestamp: tsIso });
      } else if (p.type === "function_call") {
        let input = {};
        try {
          input = JSON.parse(p.arguments || "{}");
        } catch (_) {
          input = { raw: p.arguments };
        }
        if (input && input.cmd && !input.command) input.command = input.cmd;
        const name = typeof p.name === "string" ? p.name.trim() : "";
        if (isInternalOrchestrationCall(name) || !hasUsefulFunctionCall(name, input, p.arguments)) continue;
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        const canonical = canonicalToolFields(name, input);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name: name || "tool", id: callId, input, ...canonical } }],
          ts,
          timestamp: tsIso
        });
      } else if (p.type === "function_call_output" || p.type === "custom_tool_call_output") {
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        const pending = p.type === "custom_tool_call_output" ? commandPairing.pending.get(callId) : void 0;
        if (pending) commandPairing.pending.delete(callId);
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: {
            content: codexToolOutputText(p.output),
            tool_use_id: callId,
            ...pending ? commandPairingResult(pending) : {}
          } }],
          ts,
          timestamp: tsIso
        });
      } else if (p.type === "reasoning") {
        pushOnce(reasoningResponseRow(p, ts, tsIso));
      } else if (p.type === "agent_message") {
        pushOnce(agentMessageRow(p, ts, tsIso));
      } else if (p.type === "custom_tool_call") {
        let name = typeof p.name === "string" ? p.name.trim() : "";
        if (!name) continue;
        if (isInternalOrchestrationCall(name)) continue;
        let input = {};
        if (name === "apply_patch" && typeof p.input === "string") {
          input = { patch: p.input };
        } else if (typeof p.input === "string") {
          try {
            input = JSON.parse(p.input);
          } catch (_) {
            input = { raw: p.input };
          }
        } else if (p.input && typeof p.input === "object" && !Array.isArray(p.input)) {
          input = p.input;
        }
        const nestedPatch = name === "exec" && typeof input.raw === "string" ? nestedApplyPatch(input.raw) : null;
        if (nestedPatch) {
          name = "apply_patch";
          input = { patch: nestedPatch };
        }
        const callId = typeof p.call_id === "string" ? p.call_id : "";
        const canonical = canonicalToolFields(name, input);
        const message = {
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name, id: callId, input, ...canonical } }],
          ts,
          timestamp: tsIso
        };
        const commands = name === "exec" && callId && typeof input.raw === "string" ? execScriptCommands(input.raw) : [];
        out.push(message);
        if (commands.length > 0) {
          const turnId = callTurnId(p);
          if (!turnId) continue;
          commandPairing.pending.delete(callId);
          commandPairing.pending.set(callId, {
            commandCount: commands.length,
            commands,
            completedCommands: /* @__PURE__ */ new Set(),
            commandResults: [],
            turnId
          });
          trimPendingCalls(commandPairing);
        }
      }
    }
    if (commandPairing.sessionKey && commandPairing.pending.size === 0) {
      commandPairingSessions.delete(commandPairing.sessionKey);
    }
    stampRows(lines.length - 1);
    return { messages: out };
  },
  detectSessionId(cwd, exclude, maxAgeMs) {
    return detectSessionId(cwd, exclude, maxAgeMs);
  },
  checkIntegration() {
    return checkIntegration();
  },
  installIntegration(apiPort, options) {
    return installIntegration(apiPort, options);
  },
  // Codex is the only hash authority; the grant lane never computes a trust hash CodeTerm-side.
  grantIntegrationTrust(managedKeys) {
    return grantIntegrationTrust(managedKeys);
  },
  uninstallIntegration() {
    return uninstallIntegration();
  },
  restoreIntegration() {
    return restoreIntegration();
  },
  ensureIntegrationOnStartup(apiPort) {
    return ensureIntegrationOnStartup(apiPort, (port, options) => installIntegration(port, options));
  },
  // Parse the `/status` TUI screen into a UsageSnapshot; gauges report "% left", snapshot stores "% used".
  parseUsage(rawText, nowMs) {
    return parseCodexUsage(rawText, nowMs);
  },
  fetchUsage() {
    return fetchCodexUsage(host);
  },
  detectActiveSession(_cwd, _currentSid) {
    return null;
  },
  enumerateSessions() {
    return enumerateSessions();
  },
  findSession(sessionId) {
    return findSession(sessionId);
  },
  // Read the exact model identity from this session's latest turn context.
  detectSessionModel(cwd, sessionId) {
    return detectSessionModel(cwd, sessionId);
  },
  detectSessionReasoningEffort(cwd, sessionId) {
    return detectSessionReasoningEffort(cwd, sessionId);
  },
  // Mirror native parse_session_messages for response_item message blocks.
  parseSessionMessages(text5) {
    const out = [];
    const lines = String(text5 || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (!line) continue;
      let v;
      try {
        v = JSON.parse(line);
      } catch (e) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!isValidCodexSessionMessage(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[codex-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!v || v.type !== "response_item") continue;
      const payload = v.payload;
      if (!payload || payload.type !== "message") continue;
      const role = payload.role;
      if (role !== "user" && role !== "assistant") continue;
      const content = payload.content;
      if (!Array.isArray(content)) continue;
      const chunks = [];
      for (let j = 0; j < content.length; j++) {
        const b = content[j];
        if (!b) continue;
        if ((b.type === "input_text" || b.type === "output_text" || b.type === "text") && typeof b.text === "string") {
          chunks.push(b.text);
        }
      }
      const text22 = chunks.join(" ");
      if (!text22.trim()) continue;
      let ts = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      const systemSubtype = classifyCodexSystemText(text22);
      if (systemSubtype) {
        out.push({ role: "system", text: text22, ts, isMeta: true, sourceSubtype: systemSubtype });
      } else {
        out.push({ role, text: text22, ts, isMeta: false });
      }
    }
    return out;
  },
  classifyTabState(snapshot) {
    return classifyCodexTabState(snapshot, host.unixNowMs());
  }
};
var commandPairingSessions = /* @__PURE__ */ new Map();
var MAX_PAIRING_SESSIONS = 32;
var MAX_PENDING_CALLS_PER_SESSION = 64;
function commandPairingState(context) {
  const sessionKey = typeof context?.session_key === "string" ? context.session_key : "";
  const offset = context?.from_offset;
  if (!sessionKey || typeof offset !== "number" || !Number.isSafeInteger(offset) || offset < 0) {
    return { pending: /* @__PURE__ */ new Map() };
  }
  let state = commandPairingSessions.get(sessionKey);
  if (state && state.lastOffset !== void 0 && offset <= state.lastOffset) {
    commandPairingSessions.delete(sessionKey);
    state = void 0;
  }
  if (!state) state = { pending: /* @__PURE__ */ new Map(), sessionKey };
  state.lastOffset = offset;
  commandPairingSessions.delete(sessionKey);
  commandPairingSessions.set(sessionKey, state);
  while (commandPairingSessions.size > MAX_PAIRING_SESSIONS) {
    const oldest = commandPairingSessions.keys().next().value;
    if (typeof oldest !== "string") break;
    commandPairingSessions.delete(oldest);
  }
  return state;
}
function trimPendingCalls(state) {
  while (state.pending.size > MAX_PENDING_CALLS_PER_SESSION) {
    const oldest = state.pending.keys().next().value;
    if (typeof oldest !== "string") break;
    state.pending.delete(oldest);
  }
}
function clearTurnPairings(state, turnId) {
  for (const [callId, call] of state.pending) {
    if (call.turnId === turnId) state.pending.delete(callId);
  }
}
function commandPairingResult(call) {
  if (call.commandResults.length === 0) return {};
  const results = [...call.commandResults].sort((left, right) => Number(left.command_index) - Number(right.command_index));
  const failed = results.find((result) => result.tool_status === "error");
  const complete = results.length === call.commandCount;
  const durations = results.map((result) => result.tool_duration_ms);
  const completeDurations = complete && durations.every((duration) => typeof duration === "number");
  const failureCode = failed && typeof failed.tool_exit_code === "number" && failed.tool_exit_code !== 0 ? failed.tool_exit_code : void 0;
  return {
    tool_wrapper_detail: results.map((result) => result.wrapper_detail).join("\n\n"),
    tool_command_results: results,
    ...failed ? { tool_status: "error", is_error: true } : complete ? { tool_status: "success", is_error: false } : {},
    ...failureCode !== void 0 ? { tool_exit_code: failureCode } : complete ? { tool_exit_code: 0 } : {},
    ...completeDurations ? { tool_duration_ms: durations.reduce((sum, duration) => sum + Number(duration), 0) } : {}
  };
}
function callTurnId(payload) {
  if (typeof payload.turn_id === "string") return payload.turn_id;
  const metadata = payload.internal_chat_message_metadata_passthrough;
  if (!metadata || typeof metadata !== "object" || Array.isArray(metadata)) return void 0;
  const turnId = metadata.turn_id;
  return typeof turnId === "string" && turnId ? turnId : void 0;
}
function pairCommandExecution(item, turnId, state) {
  if (!item || typeof item !== "object" || Array.isArray(item)) return false;
  const execution = item;
  if (execution.type !== "CommandExecution") return false;
  const wrapper = unwrapCommandExecution(execution.command);
  const currentTurn = typeof turnId === "string" ? turnId : "";
  if (!wrapper || !currentTurn) return false;
  const candidates = Array.from(state.pending.values()).filter((call2) => call2.turnId === currentTurn && call2.commands.some((command, index) => command === wrapper.command && !call2.completedCommands.has(index)));
  if (candidates.length !== 1) return false;
  const call = candidates[0];
  const commandIndex = call.commands.findIndex((command, index) => command === wrapper.command && !call.completedCommands.has(index));
  const activity = completedActivity(item, null, null, turnId);
  const toolData = activity?.blocks.find((block) => block.kind === "tool_use")?.data;
  if (!toolData) return false;
  call.completedCommands.add(commandIndex);
  call.commandResults.push({
    command_index: commandIndex,
    command: wrapper.command,
    wrapper_detail: wrapper.detail,
    ...toolData.tool_status !== void 0 ? { tool_status: toolData.tool_status } : {},
    ...toolData.tool_exit_code !== void 0 ? { tool_exit_code: toolData.tool_exit_code } : {},
    ...toolData.tool_duration_ms !== void 0 ? { tool_duration_ms: toolData.tool_duration_ms } : {}
  });
  return true;
}
function nestedApplyPatch(raw) {
  const call = /\btools\.apply_patch\(\s*([A-Za-z_$][\w$]*|"(?:\\.|[^"\\])*")\s*\)/.exec(raw);
  if (!call) return null;
  let literal = call[1];
  if (literal.charAt(0) !== '"') {
    const escaped = literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const declaration = new RegExp(`\\b(?:const|let)\\s+${escaped}\\s*=\\s*("(?:\\\\.|[^"\\\\])*")\\s*;`).exec(raw);
    if (!declaration) return null;
    literal = declaration[1];
  }
  try {
    const patch = JSON.parse(literal);
    return typeof patch === "string" && patch.includes("*** Begin Patch") && patch.includes("*** End Patch") ? patch : null;
  } catch (_) {
    return null;
  }
}
function canonicalToolFields(name, input) {
  const fields = baseToolFields(name, input);
  return { ...fields, tool_title: actionTitle(fields) };
}
function baseToolFields(name, input) {
  const raw = typeof input.raw === "string" ? input.raw : "";
  const command = typeof input.command === "string" ? input.command : typeof input.cmd === "string" ? input.cmd : "";
  const script = name === "exec" && raw ? summarizeExecScript(raw) : null;
  const shell = raw ? unwrapShellCommand(raw) : null;
  const patch = typeof input.patch === "string" ? input.patch : name === "apply_patch" && typeof input.input === "string" ? input.input : "";
  const edits = patch ? patchEdits(patch) : [];
  if (script) return { tool_kind: script.kind, tool_summary: script.summary, tool_detail: script.detail };
  if (shell) return { tool_kind: "command", tool_summary: shell.command, tool_detail: shell.command };
  if (command.trim()) return { tool_kind: "command", tool_summary: command.trim(), tool_detail: command.trim() };
  if (edits.length > 0) return { tool_kind: "edit", tool_summary: edits[0].path, tool_detail: patch, tool_edits: edits };
  if (patch.indexOf("*** Begin Patch") >= 0) return { tool_kind: "edit", tool_summary: "Apply patch", tool_detail: patch, tool_edits: [] };
  return { tool_kind: "generic", tool_summary: name || "Tool call", tool_detail: name || "Tool call" };
}
function isValidCodexSessionDelta(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type === "event_msg") {
    if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
    return v.payload.type === "patch_apply_end" || v.payload.type === "item_completed" || v.payload.type === "web_search_end" || v.payload.type === "image_generation_end" || v.payload.type === "task_started" || v.payload.type === "task_complete" || v.payload.type === "turn_aborted" || v.payload.type === "context_compacted" || v.payload.type === "thread_goal_updated" && sessionIntentFromPayload(v.payload) !== null || v.payload.type === "sub_agent_activity";
  }
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  const kind = v.payload.type;
  if (kind !== "message" && kind !== "reasoning" && kind !== "agent_message" && kind !== "function_call" && kind !== "function_call_output" && kind !== "custom_tool_call" && kind !== "custom_tool_call_output" && kind !== "context_compacted" && kind !== "thread_goal_updated") {
    return false;
  }
  return true;
}
function cleanIntentValue(value) {
  if (typeof value !== "string") return void 0;
  const cleaned = value.trim();
  return cleaned && !/[\u0000-\u001f\u007f]/.test(cleaned) ? cleaned : void 0;
}
function sessionIntentFromPayload(payload) {
  const title = cleanIntentValue(payload.title) || cleanIntentValue(payload.thread_name);
  const goal = cleanIntentValue(payload.goal) || cleanIntentValue(payload.thread_goal);
  if (!title && !goal) return null;
  return {
    ...title ? { title } : {},
    ...goal ? { goal } : {}
  };
}
function codexToolOutputText(output) {
  if (typeof output === "string") return output;
  if (Array.isArray(output)) {
    const text5 = [];
    for (let i = 0; i < output.length; i++) {
      const item = output[i];
      if (item && typeof item === "object" && typeof item.text === "string") {
        text5.push(item.text);
      }
    }
    if (text5.length > 0) return text5.join("");
  }
  if (output == null) return "";
  return typeof output === "object" ? JSON.stringify(output) : String(output);
}
function validateEntry(v) {
  return isValidCodexSessionDelta(v);
}
function hasUsefulFunctionCall(name, input, args) {
  if (name) return true;
  if (typeof args === "string" && args.trim() && args.trim() !== "{}") return true;
  return Object.keys(input || {}).length > 0;
}
function isInternalOrchestrationCall(name) {
  return name === "send_message" || name === "spawn_agent" || name === "followup_task" || name === "interrupt_agent" || name === "list_agents";
}
function classifyCodexSystemText(text5) {
  const trimmed = String(text5 || "").trim();
  if (!trimmed) return null;
  if (trimmed.indexOf("<turn_aborted>") === 0 && trimmed.indexOf("</turn_aborted>") !== -1) {
    return "turn_aborted";
  }
  if (/^# AGENTS\.md instructions for /m.test(trimmed) && trimmed.indexOf("<INSTRUCTIONS>") !== -1) {
    return "agent_instructions";
  }
  if (/^<environment_context(?:\s[^>]*)?>[\s\S]*<\/environment_context>$/.test(trimmed)) {
    return "environment_context";
  }
  return null;
}
function isValidCodexSessionMessage(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  if (v.payload.type !== "message") return false;
  if (v.payload.role !== "user" && v.payload.role !== "assistant") return false;
  if (!Array.isArray(v.payload.content)) return false;
  return true;
}
var plugin_default = plugin;
