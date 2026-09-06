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
  codexProfileTabId: () => codexProfileTabId,
  codexProfilesToSweep: () => codexProfilesToSweep,
  default: () => plugin_default,
  sweepCodexProfiles: () => sweepCodexProfiles
});
module.exports = __toCommonJS(plugin_exports);

// plugins/provider-codex/src/models.ts
function discoverCodexModels() {
  return discoverCodexModelsFromCache() || manifestModels();
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
      description: typeof item.description === "string" && item.description.trim() ? item.description.trim() : void 0
    });
  }
  return models.length ? models : null;
}
function priorityOf(m) {
  return typeof m.priority === "number" ? m.priority : 1e9;
}
function manifestModels() {
  try {
    const m = JSON.parse(host.manifestJson() || "null");
    const raw = Array.isArray(m && m.models) ? m.models : [];
    const kept = [];
    for (let i = 0; i < raw.length; i++) {
      const item = raw[i];
      if (item.visibility === "hide") continue;
      kept.push({ id: item.id, displayName: item.displayName, description: item.description });
    }
    return kept;
  } catch (_) {
    return [];
  }
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

// plugins/provider-codex/src/prompt.ts
var DOWN = "G1tC";
var UP = "G1tB";
var ENTER = "DQ==";
var MENU_OPTION_RE = /^[ \t]*([>❯▶➤›])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/;
function parseCodexPrompt(screen) {
  const parsed = parseCodexPromptShape(screen);
  return parsed ? parsed.prompt : null;
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
    if (/\b(?:press\s+)?enter\s+to\b/i.test(lines[i])) {
      footerLine = i;
      break;
    }
  }
  if (footerLine === -1 || lastMeaningfulLineIndex(lines) > footerLine) return null;
  return { question: questionForMenu(lines, options[0].line), options };
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
    if ((t.indexOf("esc") !== -1 || t.indexOf("enter") !== -1) && (t.indexOf("confirm") !== -1 || t.indexOf("go back") !== -1 || t.indexOf("cancel") !== -1 || t.indexOf("continue") !== -1)) return true;
  }
  return false;
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
function parseCodexLetteredPrompt(text) {
  const lines = prosePromptLines(text);
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
function parseCodexNumberedQuestion(text) {
  const lines = prosePromptLines(text);
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
function normalizeLines(text) {
  return String(text || "").replace(/\r\n?/g, "\n").split("\n");
}
function lastMeaningfulLineIndex(lines) {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line || /^[-─━═]{8,}$/.test(line.replace(/\s/g, ""))) continue;
    return i;
  }
  return -1;
}
function prosePromptLines(text) {
  const lines = normalizeLines(text);
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
function findQuestion(text) {
  const lines = normalizeLines(text).map((line) => line.trim()).filter(Boolean);
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
  const text = raw.split("\r").join("").split("\u2588").join(" ").split("\u2591").join(" ");
  const lines = text.split("\n");
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
function codexLimitFamily(text, sectionHeader) {
  const lower = String(text || "").toLowerCase();
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
  const commandBodies = [
    ...callObjects(raw, "exec_command"),
    ...callObjects(raw, "shell_command")
  ];
  const commands = commandBodies.map((body) => quotedValue(body, "cmd") || quotedValue(body, "command") || "").filter(Boolean);
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
  if (update.confidence >= CLASSIFY_THRESHOLD && update.installKeys && update.installKeys.length > 0 && update.skipKeys && update.skipKeys.length > 0) {
    return {
      state: "update_prompt",
      confidence: update.confidence,
      action: {
        providerUpdate: {
          installKeys: update.installKeys,
          declineKeys: update.skipKeys
        }
      }
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
function navigationKeys(options, matches) {
  if (options.length === 0) return null;
  let highlightIdx = -1;
  for (let i = 0; i < options.length; i++) {
    if (options[i].highlighted) {
      highlightIdx = i;
      break;
    }
  }
  if (highlightIdx === -1) highlightIdx = 0;
  let targetIdx = -1;
  for (let i = 0; i < options.length; i++) {
    if (matches(options[i].label)) {
      targetIdx = i;
      break;
    }
  }
  if (targetIdx === -1) return null;
  const keys = [];
  const delta = targetIdx - highlightIdx;
  if (delta > 0) {
    for (let i = 0; i < delta; i++) keys.push("down");
  } else if (delta < 0) {
    for (let i = 0; i < -delta; i++) keys.push("up");
  }
  keys.push("enter");
  return keys;
}
function skipNavigationKeys(options) {
  return navigationKeys(
    options,
    (label) => /\bskip\b/i.test(label) && !/\bupdate\b/i.test(label)
  );
}
function scoreUpdatePrompt(screen) {
  const options = parseMenuOptions(screen);
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
  return {
    confidence,
    skipKeys: actionableMenu ? skipNavigationKeys(options) : null,
    installKeys: actionableMenu ? navigationKeys(options, (label) => /\bupdate\b/i.test(label)) : null
  };
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

// plugins/provider-codex/src/sessions.ts
var sessionStoreOverrides = {
  root: "CODETERM_CODEX_SESSIONS_ROOT",
  index: "CODETERM_CODEX_SESSION_INDEX_PATH",
  threadHistory: "CODETERM_CODEX_THREAD_HISTORY_PATH"
};
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
  const store = codexSessionStore();
  if (!store || !sessionIndexEntry(store.indexPath, sessionId)) return null;
  return null;
}
function detectSessionId(cwd, exclude, maxAgeMs) {
  if (!cwd) return null;
  const store = codexSessionStore();
  if (!store) return null;
  const entries = readSessionIndex(store.indexPath);
  if (entries.length === 0 || !host.fs.fileExists(store.threadHistoryPath)) return null;
  const excluded = new Set(Array.isArray(exclude) ? exclude : exclude ? [exclude] : []);
  const now = host.unixNowMs ? host.unixNowMs() : Date.now();
  const ageLimit = Number.isFinite(maxAgeMs) && maxAgeMs > 0 ? maxAgeMs : 3e5;
  const matching = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (excluded.has(entry.id)) continue;
    const updatedMs = sessionTimestamp(entry.updated_at);
    if (updatedMs > 0 && now - updatedMs > ageLimit) continue;
    const project = readTrustedThreadCwd(store.threadHistoryPath, entry.id);
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
    projectPath: readTrustedThreadCwd(store.threadHistoryPath, entry.id),
    title: entry.thread_name || null,
    summary: null,
    filePath: store.indexPath,
    createdTs: null,
    modifiedTs: sessionTimestamp(entry.updated_at) > 0 ? Math.floor(sessionTimestamp(entry.updated_at) / 1e3) : null
  }));
}
function detectSessionModel(_cwd, sessionId) {
  const store = codexSessionStore();
  if (!store || !host.fs.fileExists(store.threadHistoryPath)) return null;
  let rows;
  try {
    rows = host.fs.sqliteQuery(
      store.threadHistoryPath,
      "SELECT item_type, item_json FROM thread_items WHERE thread_id = ? ORDER BY rollout_ordinal ASC, created_at_ms ASC",
      [sessionId]
    );
  } catch (_) {
    return null;
  }
  for (let i = rows.length - 1; i >= 0; i--) {
    const item = parseItem(rows[i]);
    if (!item || item.type !== "turn_context") continue;
    const model = item.payload && item.payload.model;
    if (typeof model === "string" && model.trim()) return model.trim();
  }
  return null;
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
  const historyOverride = host.envGet(sessionStoreOverrides.threadHistory);
  if (indexOverride && historyOverride) {
    return { indexPath: indexOverride, threadHistoryPath: historyOverride };
  }
  const sep = host.platform() === "windows" ? "\\" : "/";
  const root = codexSessionsRoot(sep);
  if (!root) return null;
  return {
    indexPath: indexOverride || root + sep + "session_index.jsonl",
    threadHistoryPath: historyOverride || root + sep + "thread_history_1.sqlite"
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
function readTrustedThreadCwd(db, sessionId) {
  let rows;
  try {
    rows = host.fs.sqliteQuery(
      db,
      "SELECT item_type, item_json FROM thread_items WHERE thread_id = ? ORDER BY rollout_ordinal ASC, created_at_ms ASC",
      [sessionId]
    );
  } catch (_) {
    return null;
  }
  for (let i = 0; i < rows.length; i++) {
    const item = parseItem(rows[i]);
    if (!item || item.type !== "session_meta" && item.type !== "thread_meta") continue;
    const cwd = item.payload && item.payload.cwd;
    if (typeof cwd === "string" && cwd.trim()) return cwd;
  }
  return null;
}
function parseItem(row) {
  if (typeof row.item_json !== "string") return null;
  try {
    const value = JSON.parse(row.item_json);
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  } catch (_) {
    return null;
  }
}
function sessionTimestamp(value) {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
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
function runAppServerSession(requests) {
  const lines = [
    JSON.stringify({
      method: "initialize",
      id: 1,
      params: { clientInfo: { name: "codeterm", title: "CodeTerm", version: "0.0.0" } }
    }),
    JSON.stringify({ method: "initialized" })
  ];
  for (let i = 0; i < requests.length; i++) {
    const req = { method: requests[i].method, id: requests[i].id };
    if (requests[i].params !== void 0) req.params = requests[i].params;
    lines.push(JSON.stringify(req));
  }
  let raw;
  try {
    raw = host.exec(JSON.stringify({
      bin: "codex",
      args: ["app-server"],
      stdin: lines.join("\n") + "\n",
      timeoutMs: APP_SERVER_EXIT_DEADLINE_MS
    }));
  } catch (e) {
    return { kind: "failed", reason: "host.exec threw: " + String(e) };
  }
  let result;
  try {
    result = JSON.parse(raw);
  } catch (_) {
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
    } catch (_) {
      continue;
    }
    if (typeof message.id === "number") {
      responses[message.id] = { result: message.result, error: message.error };
    }
  }
  const init = responses[1];
  if (!init) {
    if (stderrIndicatesMissingAppServer(stderr)) {
      return { kind: "unavailable", reason: "codex CLI does not support app-server: " + stderr.slice(0, 400) };
    }
    return { kind: "failed", reason: "codex app-server exited before responding" + (stderr ? ": " + stderr.slice(0, 400) : "") };
  }
  if (init.error) {
    if (isMethodNotFound(init.error)) {
      return { kind: "unavailable", reason: "codex app-server does not support initialize" };
    }
    return { kind: "failed", reason: "initialize failed: " + String(init.error.message || "unknown error") };
  }
  return { kind: "ok", responses };
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
  return c.indexOf("/hook/session_start") !== -1 || c.indexOf("/hook/stop") !== -1 || c.indexOf("codeterm-session-start.ps1") !== -1;
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
function snapshotConfig(path) {
  const existed = host.fs.fileExists(path);
  const bytes = existed ? host.fs.readFile(path) : "";
  return typeof bytes === "string" ? { path, existed, bytes } : null;
}
function restoreConfig(snapshot) {
  if (snapshot.existed) return host.fs.writeFile(snapshot.path, snapshot.bytes);
  return !host.fs.fileExists(snapshot.path) || host.fs.removeFile(snapshot.path);
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
function grantCodexHookTrust(opts) {
  if (!opts.managedLabels.length) {
    return outcome({ degraded: true, reason: "no managed hook keys declared" });
  }
  if (!opts.configPath) {
    return outcome({ degraded: true, reason: "could not resolve Codex config path" });
  }
  const listSession = runAppServerSession([
    { method: "hooks/list", id: 2, params: { cwds: [opts.listCwd] } }
  ]);
  const listResult = requestOutcome(listSession, 2, "hooks/list");
  if (listResult.kind === "unavailable") return outcome({ degraded: true, reason: listResult.reason });
  if (listResult.kind === "failed") return outcome({ reason: listResult.reason });
  const managed = managedListings(collectHookListings(listResult.result), opts.hooksPath, opts.managedLabels);
  if (managed.length !== opts.managedLabels.length) {
    return outcome({
      verify_failed: true,
      reason: "hooks/list reported " + managed.length + " managed entries, expected " + opts.managedLabels.length
    });
  }
  const needingTrust = [];
  for (let i = 0; i < managed.length; i++) {
    if (managed[i].trustStatus !== "trusted") needingTrust.push(managed[i]);
  }
  if (!needingTrust.length) {
    return outcome({
      granted: true,
      entries: managed.map((listing) => ({ key: listing.key, hash: listing.currentHash }))
    });
  }
  const snapshot = snapshotConfig(opts.configPath);
  if (!snapshot) {
    return outcome({ reason: "could not snapshot Codex config before granting trust" });
  }
  const value = {};
  for (let i = 0; i < needingTrust.length; i++) {
    value[needingTrust[i].key] = { trusted_hash: needingTrust[i].currentHash };
  }
  const writeSession = runAppServerSession([
    {
      method: "config/batchWrite",
      id: 2,
      params: { edits: [{ keyPath: "hooks.state", value, mergeStrategy: "upsert" }], reloadUserConfig: true }
    },
    { method: "hooks/list", id: 3, params: { cwds: [opts.listCwd] } }
  ]);
  const failedWrite = (reason, verifyFailed) => {
    const restored = restoreConfig(snapshot);
    return outcome({ wrote: true, verify_failed: verifyFailed, restored, reason });
  };
  const writeResult = requestOutcome(writeSession, 2, "config/batchWrite");
  if (writeResult.kind !== "ok") return outcome({ reason: writeResult.reason });
  const verifyResult = requestOutcome(writeSession, 3, "hooks/list");
  if (verifyResult.kind !== "ok") return failedWrite(verifyResult.reason, true);
  const verified = managedListings(collectHookListings(verifyResult.result), opts.hooksPath, opts.managedLabels);
  if (verified.length !== opts.managedLabels.length) {
    return failedWrite("post-grant verify reported " + verified.length + " managed entries", true);
  }
  const expectedHashes = {};
  for (let i = 0; i < managed.length; i++) {
    expectedHashes[managed[i].key] = managed[i].currentHash;
  }
  for (let i = 0; i < verified.length; i++) {
    const listing = verified[i];
    if (listing.trustStatus !== "trusted") {
      return failedWrite("post-grant verify left " + listing.key + " " + listing.trustStatus, true);
    }
    if (expectedHashes[listing.key] !== listing.currentHash) {
      return failedWrite("post-grant verify reported a different hash for " + listing.key, true);
    }
  }
  return outcome({
    granted: true,
    wrote: true,
    entries: verified.map((listing) => ({ key: listing.key, hash: listing.currentHash }))
  });
}

// plugins/provider-codex/src/hooks.ts
var CODETERM_CODEX_HOOK_MARKER = "codeterm-codex-session-binding-v1";
var CODETERM_CODEX_PROFILE_CLEANUP_MARKER = "codeterm-profile-cleanup-v1";
function codexWindowsHookScript() {
  return [
    "# " + CODETERM_CODEX_HOOK_MARKER,
    "# " + CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
    'param([string]$HookEvent = "session_start")',
    '$ErrorActionPreference = "SilentlyContinue"',
    "$tab = $env:CODETERM_TAB_ID",
    "if ([string]::IsNullOrWhiteSpace($tab) -or [string]::IsNullOrWhiteSpace($env:CODETERM_API_URL) -or [string]::IsNullOrWhiteSpace($env:CODETERM_API_TOKEN)) { exit 0 }",
    '$base = $env:CODETERM_API_URL.TrimEnd("/")',
    "$headers = @{",
    '  "Content-Type" = "application/json"',
    '  "X-CodeTerm-Agent-Provider" = "codex"',
    "}",
    "if ($env:CODETERM_SESSION_BINDING_NONCE) {",
    '  $headers["X-CodeTerm-Binding-Nonce"] = $env:CODETERM_SESSION_BINDING_NONCE',
    "}",
    '$headers["Authorization"] = "Bearer $($env:CODETERM_API_TOKEN)"',
    "$body = [Console]::In.ReadToEnd()",
    'Invoke-RestMethod -Method Post -Uri "$base/api/tabs/$tab/hook/$HookEvent" -Headers $headers -Body $body -TimeoutSec 5 | Out-Null',
    'if ($HookEvent -eq "session_start") {',
    "  $profileSuffix = $tab -replace '[^A-Za-z0-9_-]', '-'",
    '  $profilePath = Join-Path $env:USERPROFILE (".codex\\codeterm-$profileSuffix.config.toml")',
    "  Remove-Item -LiteralPath $profilePath -Force -ErrorAction SilentlyContinue",
    "}",
    "exit 0",
    ""
  ].join("\n");
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
  if (hook._source !== void 0) return hook._source === CODETERM_CODEX_HOOK_MARKER;
  const cmd = String(hook.command || "");
  return (cmd.indexOf("/hook/session_start") !== -1 || cmd.indexOf("/hook/stop") !== -1) && cmd.indexOf("X-CodeTerm-Agent-Provider: codex") !== -1;
}
var CODEX_HOOK_SCRIPT_CONTRACT = [
  CODETERM_CODEX_HOOK_MARKER,
  CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
  "CODETERM_TAB_ID",
  "CODETERM_API_URL",
  "CODETERM_API_TOKEN",
  "CODETERM_SESSION_BINDING_NONCE",
  "/api/tabs/"
];
function codexHookContentHash(text) {
  let hash = 2166136261;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24)) >>> 0;
  }
  return ("0000000" + hash.toString(16)).slice(-8);
}
function isOwnedCodexHookScriptContent(text) {
  return text.indexOf(CODETERM_CODEX_HOOK_MARKER) !== -1;
}
function isCurrentCodexHookScriptContent(text) {
  if (!isOwnedCodexHookScriptContent(text)) return false;
  if (/:\d{2,5}/.test(text)) return false;
  for (let i = 0; i < CODEX_HOOK_SCRIPT_CONTRACT.length; i++) {
    if (text.indexOf(CODEX_HOOK_SCRIPT_CONTRACT[i]) === -1) return false;
  }
  return true;
}
function isOwnedCodexWindowsHookScript(path) {
  const text = host.fs.readFile(path);
  return typeof text === "string" && isCurrentCodexHookScriptContent(text);
}

// plugins/provider-codex/src/integration.ts
var CODEX_INTEGRATION_STATE_ABSENT = "absent";
var CODEX_INTEGRATION_STATE_STALE_VERSION = "stale_version";
var CODEX_INTEGRATION_STATE_UNTRUSTED_HASH = "untrusted_hash";
var CODEX_INTEGRATION_STATE_INSTALLED = "installed";
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
  const windowsReady = host.platform() !== "windows" || !!winScript && winScriptText.indexOf(CODETERM_CODEX_PROFILE_CLEANUP_MARKER) !== -1 && Array.isArray(arr) && arr.some(hasCodetermCodexWindowsCommand);
  const contractOk = host.platform() === "windows" ? winScriptText.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 && winScriptText.indexOf("CODETERM_TAB_ID") !== -1 && winScriptText.indexOf("CODETERM_API_URL") !== -1 && winScriptText.indexOf("CODETERM_API_TOKEN") !== -1 && !hasLiteralPort(winScriptText) && winScriptText.indexOf("/api/tabs/") !== -1 : Array.isArray(arr) && arr.some((entry) => {
    const hk = entry && entry.hooks;
    return Array.isArray(hk) && hk.some((h) => {
      const command = String(h?.command || "");
      return command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 && command.indexOf("CODETERM_TAB_ID") !== -1 && command.indexOf("CODETERM_API_URL") !== -1 && command.indexOf("CODETERM_API_TOKEN") !== -1 && !hasLiteralPort(command) && command.indexOf("/api/tabs/") !== -1;
    });
  });
  const current = installed && windowsReady && contractOk;
  const trustState = codexHookTrustState(path);
  const trustOk = trustState !== "disabled";
  const integrationState = !installed ? CODEX_INTEGRATION_STATE_ABSENT : !trustOk || trustState === "missing" || trustState === "unknown" ? CODEX_INTEGRATION_STATE_UNTRUSTED_HASH : !current || !stopInstalled ? CODEX_INTEGRATION_STATE_STALE_VERSION : CODEX_INTEGRATION_STATE_INSTALLED;
  return {
    hooks_file_exists: true,
    hook_content_hash: ownedContentHash(arr, stopArr, winScriptText),
    hooks_installed: current && stopInstalled && trustOk,
    session_start_hook: current && trustOk,
    stop_hook: stopInstalled && trustOk,
    hook_trust_state: trustState,
    hook_trust_enabled: trustState === "enabled" ? true : trustState === "disabled" ? false : null,
    integration_state: integrationState,
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
    return { ok: false, error: restored ? error : error + "; exact rollback failed" };
  };
  if (!host.fs.writeFile(path + ".codeterm-backup", originalHooks) || !host.fs.writeFile(path + ".codeterm-backup-state", hooksExisted ? "present" : "absent")) {
    return fail("could not write recoverable backup for " + path);
  }
  if (configPath && configExisted && !host.fs.writeFile(configPath + ".codeterm-backup", originalConfig)) {
    return fail("could not write recoverable backup for " + configPath);
  }
  if (scriptPath && (!host.fs.writeFile(scriptPath + ".codeterm-backup", originalScript) || !host.fs.writeFile(scriptPath + ".codeterm-backup-state", scriptExisted ? "present" : "absent"))) {
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
  const guard = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then ';
  const hookCmd = (route) => guard + 'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/' + route + `" -H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' ` + nonceH + " " + authH + " --data-binary @- > /dev/null 2>&1 || true; fi";
  const winCmd = (route) => scriptPath ? 'powershell -NoProfile -ExecutionPolicy Bypass -File "' + scriptPath + '" -HookEvent ' + route : hookCmd(route);
  if (scriptPath && !host.fs.writeFile(scriptPath, codexWindowsHookScript())) {
    return fail("writeFile failed for " + scriptPath);
  }
  json.hooks.SessionStart.push({
    matcher: "startup|resume|clear|compact",
    hooks: [{
      type: "command",
      command: hookCmd("session_start"),
      commandWindows: winCmd("session_start"),
      timeout: 5,
      _source: CODETERM_CODEX_HOOK_MARKER
    }]
  });
  json.hooks.Stop.push({
    hooks: [{
      type: "command",
      command: hookCmd("stop"),
      commandWindows: winCmd("stop"),
      timeout: 5,
      _source: CODETERM_CODEX_HOOK_MARKER
    }]
  });
  const ok = host.fs.writeJson(path, json);
  if (ok) {
    const trust = enableExistingCodexHookTrust(path);
    if (trust.write_failed) return fail("could not enable existing Codex hook trust");
    if (trust.changed) {
      if (!ownershipPath || !host.fs.writeFile(ownershipPath, path)) {
        return fail("could not record owned Codex hook trust expansion");
      }
    }
    host.log("info", "[codex-plugin] SessionStart + Stop hooks installed at " + path);
    return { ok: true, hook_trust_state: trust.state };
  }
  return fail("writeJson failed for " + path);
}
function grantIntegrationTrust(managedKeys) {
  const hooksPath = codexHooksPath();
  if (!hooksPath) {
    return { granted: false, wrote: false, degraded: true, verify_failed: false, reason: "could not resolve ~/.codex/hooks.json" };
  }
  return grantCodexHookTrust({
    hooksPath,
    configPath: codexConfigPath(),
    managedLabels: Array.isArray(managedKeys) ? managedKeys.map(String) : [],
    listCwd: host.homeDir() || "."
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
    if (Array.isArray(json.hooks.SessionStart)) {
      json.hooks.SessionStart = removeOwnedCodexHooks(json.hooks.SessionStart);
    }
    if (Array.isArray(json.hooks.Stop)) {
      json.hooks.Stop = removeOwnedCodexHooks(json.hooks.Stop);
    }
    hooksOk = host.fs.writeJson(path, json);
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
  } else if (!host.fs.writeFile(path, hooks)) {
    return { ok: false, error: "restore failed for " + path };
  }
  const config = codexConfigPath();
  if (config && host.fs.fileExists(config + ".codeterm-backup")) {
    const original = host.fs.readFile(config + ".codeterm-backup");
    if (typeof original !== "string" || !host.fs.writeFile(config, original)) return { ok: false, error: "restore failed for " + config };
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
    } else if (!host.fs.writeFile(script, original)) {
      return { ok: false, error: "restore failed for " + script };
    }
  }
  return { ok: true };
}
function ensureIntegrationOnStartup(apiPort, install) {
  const path = codexHooksPath();
  if (!path || !host.fs.fileExists(path)) return { ok: true, changed: false };
  const json = host.fs.readJson(path);
  if (!json || !json.hooks) return { ok: true, changed: false };
  const owned = (json.hooks.SessionStart || []).filter(isCodetermCodexHook);
  if (owned.length === 0) return { ok: true, changed: false };
  if (codexHookTrustState(path) === "disabled") {
    return { ok: true, changed: false, reason: "Codex hook trust is disabled" };
  }
  const scriptPath = codexWindowsHookScriptPath();
  const scriptStale = host.platform() === "windows" && !!scriptPath && (!host.fs.fileExists(scriptPath) || !isOwnedCodexWindowsHookScript(scriptPath));
  const commandStale = owned.some((entry) => {
    const hooks = entry && entry.hooks || [];
    return hooks.some(
      (hook) => {
        const command = String(hook && hook.command || "");
        return command.indexOf("$CODETERM_API_URL") === -1 || command.indexOf("${CODETERM_API_TOKEN}") === -1 || command.indexOf("$CODETERM_TAB_ID") === -1 || hasLiteralPort(command);
      }
    );
  });
  if (!scriptStale && !commandStale) return { ok: true, changed: false };
  const result = install(apiPort, { consent: true });
  return result && result.ok ? { ok: true, changed: true, reason: "refreshed a stale CodeTerm-owned hook" } : { ok: false, changed: false, error: result && result.error || "refresh failed" };
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
  return home + sep + ".codex" + sep + "codeterm-session-start.ps1";
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
function restoreIntegrationFiles(snapshots) {
  let restored = true;
  for (let i = snapshots.length - 1; i >= 0; i--) {
    const snapshot = snapshots[i];
    const ok = snapshot.existed ? host.fs.writeFile(snapshot.path, snapshot.bytes) : !host.fs.fileExists(snapshot.path) || host.fs.removeFile(snapshot.path);
    restored = ok && restored;
  }
  return restored;
}
function codexHookTrustState(hooksPath) {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return "missing";
  const text = String(host.fs.readFile(cfg) || "");
  if (!text) return "missing";
  const block = findCodexHookTrustBlock(text, hooksPath);
  if (block == null) return "missing";
  const m = block.match(/(?:^|\n)\s*enabled\s*=\s*(true|false)\s*(?:\n|$)/);
  if (m) return m[1] === "true" ? "enabled" : "disabled";
  if (/(?:^|\n)\s*trusted_hash\s*=/.test(block)) return "trusted";
  return "unknown";
}
function enableExistingCodexHookTrust(hooksPath) {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return { state: "missing", changed: false, write_failed: false };
  const text = String(host.fs.readFile(cfg) || "");
  if (!text) return { state: "missing", changed: false, write_failed: false };
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  if (!span) return { state: "missing", changed: false, write_failed: false };
  const block = text.substring(span.start, span.end);
  if (!/(?:^|\n)\s*enabled\s*=\s*false\s*(?:\n|$)/.test(block)) {
    const current = codexHookTrustState(hooksPath);
    const state = current === "enabled" || current === "trusted" ? current : "unknown";
    return { state, changed: false, write_failed: false };
  }
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)false(\s*(?:\n|$))/, "$1$2true$3");
  const next = text.substring(0, span.start) + nextBlock + text.substring(span.end);
  return host.fs.writeFile(cfg, next) ? { state: "enabled", changed: true, write_failed: false } : { state: "unknown", changed: false, write_failed: true };
}
function disableOwnedCodexHookTrust(hooksPath) {
  const cfg = codexConfigPath();
  if (!cfg || !host.fs.fileExists(cfg)) return true;
  const text = host.fs.readFile(cfg);
  if (typeof text !== "string") return false;
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  if (!span) return true;
  const block = text.substring(span.start, span.end);
  const nextBlock = block.replace(/(^|\n)(\s*enabled\s*=\s*)true(\s*(?:\n|$))/, "$1$2false$3");
  if (nextBlock === block) return true;
  const next = text.substring(0, span.start) + nextBlock + text.substring(span.end);
  return host.fs.writeFile(cfg, next);
}
function findCodexHookTrustBlock(text, hooksPath) {
  const span = findCodexHookTrustBlockSpan(text, hooksPath);
  return span ? text.substring(span.start, span.end) : null;
}
function findCodexHookTrustBlockSpan(text, hooksPath) {
  const marker = hooksPath + ":session_start:";
  const lines = text.split("\n");
  let offset = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    const isHeader = trimmed.charAt(0) === "[" && trimmed.charAt(trimmed.length - 1) === "]";
    if (isHeader && trimmed.indexOf("hooks.state.") !== -1 && trimmed.indexOf(marker) !== -1) {
      const start = offset;
      let end = text.length;
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
function hasCodetermCodexWindowsCommand(entry) {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const hook = hooks[i];
    if (!hook || hook._source !== CODETERM_CODEX_HOOK_MARKER) continue;
    const command = String(hook.commandWindows || "");
    if (command.indexOf("codeterm-session-start.ps1") !== -1) {
      return true;
    }
  }
  return false;
}

// plugins/provider-codex/src/profile-sweep.ts
var CODEX_PROFILE_NAME_RE = /^codeterm-([A-Za-z0-9_-]+)\.config\.toml$/;
function codexProfileTabId(fileName) {
  const m = CODEX_PROFILE_NAME_RE.exec(fileName);
  return m ? m[1] : null;
}
function codexProfilesToSweep(fileNames, liveTabIds) {
  const out = [];
  for (const name of fileNames) {
    const tabId = codexProfileTabId(name);
    if (tabId === null) continue;
    if (!liveTabIds.has(tabId)) out.push(name);
  }
  return out;
}
function sweepCodexProfiles(host2, liveTabIds) {
  if (host2.platform() !== "windows") return { removed: [] };
  const home = host2.homeDir();
  if (!home) return { removed: [] };
  const dir = home + "\\.codex";
  let entries;
  try {
    entries = host2.fs.readDir(dir);
  } catch {
    return { removed: [] };
  }
  if (!Array.isArray(entries)) return { removed: [] };
  const names = entries.filter((e) => e && e.isFile).map((e) => e.name);
  const toRemove = codexProfilesToSweep(names, new Set(liveTabIds));
  const removed = [];
  for (const name of toRemove) {
    if (host2.fs.removeFile(dir + "\\" + name)) removed.push(name);
  }
  return { removed };
}

// plugins/provider-codex/src/plugin.ts
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
  detectFromOutput(text) {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },
  screenHasTui(screen) {
    const text = String(screen || "");
    const lower = text.toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return hasPromptAndStatusFooter(text);
  },
  isOutputNoise(text) {
    const trimmed = String(text || "").trim();
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
  detectEvents(text) {
    const t = String(text || "");
    const events = [];
    if (t.indexOf("Would you like to run the following command") !== -1 || t.indexOf("Would you like to make the following edits") !== -1 || t.indexOf("Do you want to approve network access") !== -1 || t.indexOf("Would you like to grant these permissions") !== -1) {
      events.push("permission_request");
    }
    return events;
  },
  parsePrompt(text) {
    return parseCodexPrompt(String(text || ""));
  },
  buildPromptResponse(optionIndex, screen) {
    return responseKeysForIndex(optionIndex, String(screen || ""));
  },
  directoryTrustPromptResponse(prompt, screen) {
    const question = String(prompt.question || "").toLowerCase();
    const first = prompt.options[0];
    const firstLabel = Array.isArray(first) ? first[1] : first.label;
    if (!question.includes("trust the contents of this directory") || !firstLabel || !/^yes\b/i.test(firstLabel)) {
      return null;
    }
    return responseKeysForIndex(0, String(screen || ""));
  },
  // Codex CLI has no model-list subcommand; its local cache is the only sync account-scoped source. Missing => seed.
  discoverModels() {
    return discoverCodexModels();
  },
  buildLaunchCommand(params) {
    const p = params || {};
    const parts = [];
    let prefix = "";
    parts.push("codex");
    if (p.skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    if (p.task?.trim()) {
      const taskPath = host.fs.writeTempFile(p.task, ".ct-task.md");
      if (!taskPath) throw new Error("failed to write Codex task instructions");
      const quotedPath = host.shell.quoteFor(taskPath, host.platform());
      if (host.platform() === "windows") {
        prefix = `$ctTaskInstructions = Get-Content -Raw -Encoding UTF8 ${quotedPath}; $ctTaskJson = ($ctTaskInstructions | ConvertTo-Json -Compress); $ctProfileSuffix = if ($env:CODETERM_TAB_ID) { $env:CODETERM_TAB_ID -replace '[^A-Za-z0-9_-]', '-' } else { [guid]::NewGuid().ToString('N') }; $ctProfileName = 'codeterm-' + $ctProfileSuffix; $ctProfilePath = Join-Path $env:USERPROFILE ('.codex\\' + $ctProfileName + '.config.toml'); Set-Content -LiteralPath $ctProfilePath -Encoding UTF8 -Value ('developer_instructions = ' + $ctTaskJson); try { `;
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
        return prefix + parts.join(" ") + " } finally { Remove-Item -LiteralPath $ctProfilePath -Force -ErrorAction SilentlyContinue }";
      }
    }
    return prefix + parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const parts = ["codex", "resume"];
    if (skipPermissions) {
      parts.push("--dangerously-bypass-approvals-and-sandbox");
    }
    const uuid = extractSessionUuid(sessionId) || sessionId;
    parts.push(host.shell.quoteFor(uuid, host.platform()));
    return parts.join(" ");
  },
  sessionFilePath(_cwd, sessionId) {
    return sessionFilePath(_cwd, sessionId);
  },
  // Codex 0.153 has no per-session JSONL transcript; keep the path capability honest.
  sessionJsonlPath(cwd, sessionId) {
    return this.sessionFilePath(cwd, sessionId);
  },
  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts.
  parseSessionDelta(chunk) {
    const out = [];
    const lines = String(chunk || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
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
      if (v.type === "event_msg" && v.payload && v.payload.type === "patch_apply_end") {
        const p2 = v.payload;
        const content = typeof p2.stdout === "string" ? p2.stdout : "";
        const callId = typeof p2.call_id === "string" ? p2.call_id : "";
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content, tool_use_id: callId } }],
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
        let text = "";
        if (Array.isArray(p.content)) {
          for (let j = 0; j < p.content.length; j++) {
            const c = p.content[j];
            if (c && typeof c.text === "string") text += c.text;
          }
        } else if (typeof p.content === "string") {
          text = p.content;
        }
        if (!text) continue;
        const systemSubtype = classifyCodexSystemText(text);
        if (systemSubtype) {
          out.push({
            role: "system",
            blocks: [{ kind: "text", data: { text, subtype: systemSubtype } }],
            sourceType: v.type,
            sourceSubtype: systemSubtype,
            ts,
            timestamp: tsIso
          });
          continue;
        }
        out.push({ role, blocks: [{ kind: "text", data: { text } }], ts, timestamp: tsIso });
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
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_result", data: { content: codexToolOutputText(p.output), tool_use_id: callId } }],
          ts,
          timestamp: tsIso
        });
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
        out.push({
          role: "assistant",
          blocks: [{ kind: "tool_use", data: { name, id: callId, input, ...canonical } }],
          ts,
          timestamp: tsIso
        });
      }
    }
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
  // Read the exact model identity from this session's latest turn context.
  detectSessionModel(cwd, sessionId) {
    return detectSessionModel(cwd, sessionId);
  },
  // Mirror native parse_session_messages for response_item message blocks.
  parseSessionMessages(text) {
    const out = [];
    const lines = String(text || "").split("\n");
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
      const text2 = chunks.join(" ");
      if (!text2.trim()) continue;
      let ts = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      const systemSubtype = classifyCodexSystemText(text2);
      if (systemSubtype) {
        out.push({ role: "system", text: text2, ts, isMeta: true, sourceSubtype: systemSubtype });
      } else {
        out.push({ role, text: text2, ts, isMeta: false });
      }
    }
    return out;
  },
  classifyTabState(snapshot) {
    return classifyCodexTabState(snapshot, host.unixNowMs());
  },
  providerSelfUpdateCapability() {
    return {
      inSession: true,
      restartRequired: true,
      completionSignal: "provider_classification"
    };
  }
};
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
    return v.payload.type === "patch_apply_end";
  }
  if (v.type !== "response_item") return false;
  if (!v.payload || typeof v.payload !== "object" || Array.isArray(v.payload)) return false;
  const kind = v.payload.type;
  if (kind !== "message" && kind !== "function_call" && kind !== "function_call_output" && kind !== "custom_tool_call" && kind !== "custom_tool_call_output") {
    return false;
  }
  return true;
}
function codexToolOutputText(output) {
  if (typeof output === "string") return output;
  if (Array.isArray(output)) {
    const text = [];
    for (let i = 0; i < output.length; i++) {
      const item = output[i];
      if (item && typeof item === "object" && typeof item.text === "string") {
        text.push(item.text);
      }
    }
    if (text.length > 0) return text.join("");
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
  return name === "send_message" || name === "spawn_agent" || name === "followup_task" || name === "interrupt_agent" || name === "list_agents" || name === "wait_agent";
}
function classifyCodexSystemText(text) {
  const trimmed = String(text || "").trim();
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
