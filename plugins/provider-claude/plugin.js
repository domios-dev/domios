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

// plugins/provider-claude/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);

// plugins/provider-claude/src/shared.ts
function manifest() {
  try {
    return JSON.parse(host.manifestJson() || "null");
  } catch (e) {
    return null;
  }
}

// plugins/provider-claude/src/usage.ts
function usageNormalize(raw) {
  let out = "";
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === "" || ch === "\r") continue;
    if ("\u2588\u258C\u258D\u258E\u258F\u258A\u258B".indexOf(ch) !== -1) {
      out += " ";
      continue;
    }
    out += ch;
  }
  return out;
}
var CLAUDE_USAGE_KIND = "codeterm_usage_v1";
function fetchClaudeUsage(host2) {
  const credentialsPath = host2.expandHome("~/.claude/.credentials.json");
  const fileCredentials = credentialsPath ? safeJson(host2.fs.readFile(credentialsPath)) : null;
  const credentials = readMacOsCredentials(host2) ?? fileCredentials;
  const oauth = objectAt(credentials, "claudeAiOauth");
  const accessToken = stringAt(oauth, "accessToken");
  if (!accessToken) throw new Error("usage credentials unavailable");
  const response = safeJson(host2.fetch(JSON.stringify({
    url: "https://api.anthropic.com/api/oauth/usage",
    method: "GET",
    timeoutMs: 6e3,
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "anthropic-beta": "oauth-2025-04-20",
      "User-Agent": "codeterm-usage/1.7.8"
    }
  })));
  const fetchError = stringAt(response, "error");
  if (fetchError) throw new Error(`usage request failed: ${fetchError}`);
  const status = numberAt(response, "status");
  if (status !== 200) throw new Error(`usage request failed: HTTP ${status ?? "unknown"}`);
  const body = safeJson(stringAt(response, "body"));
  const session = objectAt(body, "five_hour");
  const weekly = objectAt(body, "seven_day");
  if (!session && !weekly) throw new Error("usage response missing quota windows");
  return JSON.stringify({
    kind: CLAUDE_USAGE_KIND,
    session_pct: numberAt(session, "utilization"),
    session_resets_at_ms: isoToMs(stringAt(session, "resets_at")),
    weekly_pct: numberAt(weekly, "utilization"),
    weekly_resets_at_ms: isoToMs(stringAt(weekly, "resets_at"))
  });
}
function readMacOsCredentials(host2) {
  if (host2.platform() !== "macos") return null;
  const user = host2.envGet("USER");
  const args = ["find-generic-password"];
  if (user) args.push("-a", user);
  args.push("-w", "-s", "Claude Code-credentials");
  const result = safeJson(host2.exec(JSON.stringify({ bin: "security", args, timeoutMs: 5e3 })));
  if (numberAt(result, "code") !== 0) return null;
  return safeJson(stringAt(result, "stdout"));
}
function parseFetchedClaudeUsage(raw, nowMs) {
  const fetched = safeJson(raw);
  if (stringAt(fetched, "kind") !== CLAUDE_USAGE_KIND) return void 0;
  return {
    provider: "claude",
    account_id: null,
    captured_at_ms: nowMs,
    session_pct: numberAt(fetched, "session_pct"),
    session_resets_at_ms: numberAt(fetched, "session_resets_at_ms"),
    weekly_pct: numberAt(fetched, "weekly_pct"),
    weekly_resets_at_ms: numberAt(fetched, "weekly_resets_at_ms"),
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
function isoToMs(value) {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}
function usageBlockIsComplete(block) {
  return block.pct === null || block.resetsMs !== null;
}
function usageParseBlock(text, marker, isWeekly, nowMs) {
  const markerMatch = new RegExp(marker).exec(text);
  if (!markerMatch) return { pct: null, resetsMs: null };
  const tail = text.slice(markerMatch.index + markerMatch[0].length, markerMatch.index + markerMatch[0].length + 800);
  const nextBlock = /\n\s*Current\s+/i.exec(tail);
  const block = nextBlock ? tail.slice(0, nextBlock.index) : tail;
  const pctCaps = /(\d+(?:\.\d+)?)\s*%\s*used/.exec(block);
  if (!pctCaps) return { pct: null, resetsMs: null };
  let pct = parseFloat(pctCaps[1]);
  if (isNaN(pct)) pct = null;
  const resetCaps = /Resets?\s*([^()\n]+?)\s*\(/.exec(block);
  const timeRaw = (resetCaps?.[1] || "").trim();
  const resetsMs = isWeekly ? usageParseWeeklyReset(timeRaw, nowMs) : usageParseTodayClock(timeRaw, nowMs);
  return { pct, resetsMs };
}
function usageParseHMmAmPm(s) {
  s = String(s || "").trim().toLowerCase();
  let pm;
  if (s.slice(-2) === "pm") pm = true;
  else if (s.slice(-2) === "am") pm = false;
  else return null;
  const stripped = s.slice(0, -2).trim();
  let hh, mm;
  const idx = stripped.indexOf(":");
  if (idx !== -1) {
    hh = parseInt(stripped.slice(0, idx).trim(), 10);
    mm = parseInt(stripped.slice(idx + 1).trim(), 10);
  } else {
    hh = parseInt(stripped, 10);
    mm = 0;
  }
  if (isNaN(hh) || isNaN(mm) || hh === 0 || hh > 12 || mm > 59) return null;
  return { hh, mm, pm };
}
function usageParseTodayClock(s, nowMs) {
  const t = usageParseHMmAmPm(s);
  if (!t) return null;
  const hour24 = t.hh % 12 + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, t.mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1e3);
  return cand.getTime();
}
function usageParseWeeklyReset(s, nowMs) {
  return usageParseMonthDayClock(s, nowMs) ?? usageParseRelativeDayClock(s, nowMs) ?? usageParseTodayClock(s, nowMs);
}
var USAGE_MONTHS = {
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
function usageParseMonthDayClock(s, nowMs) {
  const parts = usageCleanParts(s);
  while (parts.length > 0 && usageIsWeekday(parts[0])) parts.shift();
  if (parts.length < 3) return null;
  const month = USAGE_MONTHS[parts[0]];
  if (month === void 0) return null;
  const day = usageParseDayToken(parts[1]);
  if (day === null) return null;
  let idx = 2;
  let explicitYear = null;
  if (/^\d{4}$/.test(parts[idx] || "")) {
    explicitYear = parseInt(parts[idx], 10);
    idx++;
  }
  if (parts[idx] === "at") idx++;
  const t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  const hour24 = t.hh % 12 + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  const year = explicitYear ?? now.getFullYear();
  let cand = new Date(year, month, day, hour24, t.mm, 0, 0);
  if (explicitYear === null && cand.getTime() < nowMs) {
    cand = new Date(year + 1, month, day, hour24, t.mm, 0, 0);
  }
  return cand.getTime();
}
function usageParseRelativeDayClock(s, nowMs) {
  const parts = usageCleanParts(s);
  const first = parts[0];
  let offsetDays;
  if (first === "today" || first === "tonight") offsetDays = 0;
  else if (first === "tomorrow") offsetDays = 1;
  else return null;
  let idx = 1;
  if (parts[idx] === "at") idx++;
  const t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  const hour24 = t.hh % 12 + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + offsetDays,
    hour24,
    t.mm,
    0,
    0
  ).getTime();
}
function usageCleanParts(s) {
  return String(s || "").trim().toLowerCase().replace(/,/g, " ").split(/\s+/).map((p) => p.replace(/[.;]+$/g, "")).filter(Boolean);
}
function usageParseDayToken(s) {
  const n = parseInt(String(s || "").replace(/(?:st|nd|rd|th)$/i, ""), 10);
  return Number.isFinite(n) && n >= 1 && n <= 31 ? n : null;
}
function usageIsWeekday(s) {
  return /^(mon|monday|tue|tues|tuesday|wed|wednesday|thu|thur|thurs|thursday|fri|friday|sat|saturday|sun|sunday)$/.test(s);
}

// plugins/provider-claude/src/session.ts
var CLAUDE_SESSION_DELTA_TYPES = { system: true, user: true, assistant: true, attachment: true };
function claudeSessionFilePath(cwd, sessionId) {
  const m = manifest();
  const root = m && m.sessions && m.sessions.rootDir || "~/.claude/projects";
  const ext = m && m.sessions && m.sessions.fileExtension || "jsonl";
  const expanded = host.expandHome(root);
  if (!expanded) return null;
  const encoded = encodeCwdDashed(cwd);
  const sep = host.platform() === "windows" ? "\\" : "/";
  return expanded + sep + encoded + sep + sessionId + "." + ext;
}
function encodeCwdDashed(cwd) {
  return String(cwd || "").replace(/[^a-zA-Z0-9]/g, "-");
}
function contentToText(c) {
  if (typeof c === "string") return c;
  if (!Array.isArray(c)) return "";
  const chunks = [];
  for (let i = 0; i < c.length; i++) {
    const b = c[i];
    if (b && typeof b.text === "string") chunks.push(b.text);
  }
  return chunks.join(" ");
}
function isValidClaudeSessionMessage(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "user" && v.type !== "assistant") return false;
  if (!v.message || typeof v.message !== "object" || Array.isArray(v.message)) return false;
  const content = v.message.content;
  if (typeof content !== "string" && !Array.isArray(content)) return false;
  return true;
}
function validateEntry(v) {
  return isValidClaudeSessionDelta(v);
}
function isValidClaudeSessionDelta(v) {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (!CLAUDE_SESSION_DELTA_TYPES[v.type]) return false;
  if (v.type === "system") {
    const sc = v.content;
    if (typeof sc === "string" || Array.isArray(sc)) return true;
    return typeof v.subtype === "string" && v.subtype.length > 0;
  }
  if (v.type === "attachment") {
    const a = v.attachment;
    return !!a && typeof a === "object" && a.type === "queued_command" && typeof a.prompt === "string" && a.prompt.length > 0;
  }
  const msg = v.message;
  if (!msg || typeof msg !== "object" || Array.isArray(msg)) return false;
  if (typeof msg.content !== "string" && !Array.isArray(msg.content)) return false;
  return true;
}
function extractSetModelFamily(raw) {
  const idx = raw.indexOf("Set model to ");
  if (idx === -1) return null;
  const after = raw.substring(idx + "Set model to ".length);
  const cleaned = stripAnsi(after).replace(/^\s+/, "");
  let fam = "";
  for (let i = 0; i < cleaned.length; i++) {
    const c = cleaned.charAt(i);
    if (!/[a-zA-Z]/.test(c)) break;
    fam += c;
  }
  return fam ? capitalize(fam) : null;
}
function stripAnsi(s) {
  let out = "";
  let i = 0;
  while (i < s.length) {
    if (s.charCodeAt(i) === 27 && s.charAt(i + 1) === "[") {
      i += 2;
      while (i < s.length && !/[a-zA-Z]/.test(s.charAt(i))) i++;
      if (i < s.length) i++;
    } else {
      out += s.charAt(i);
      i++;
    }
  }
  return out;
}
function familyFromModelId(id) {
  const m = String(id || "").toLowerCase();
  const fams = ["opus", "sonnet", "haiku"];
  for (let i = 0; i < fams.length; i++) {
    if (m.indexOf(fams[i]) !== -1) return capitalize(fams[i]);
  }
  const body = m.indexOf("claude-") === 0 ? m.substring(7) : m;
  const head = body.split("-")[0] || body;
  return capitalize(head);
}
function capitalize(s) {
  if (!s) return "";
  return s.charAt(0).toUpperCase() + s.substring(1).toLowerCase();
}

// plugins/provider-claude/src/models.ts
var FAMILIES = ["opus", "sonnet", "haiku", "fable"];
function discoverAnthropicModels() {
  const key = host.envGet("ANTHROPIC_API_KEY") || host.envGet("CLAUDE_API_KEY");
  if (!key) return null;
  const live = fetchLiveModels(key);
  if (!live) return null;
  const mapped = mapAliasesToLive(manifestModels(), live);
  return mapped.length ? mapped : null;
}
function manifestModels() {
  const m = manifest();
  return Array.isArray(m && m.models) ? m.models : [];
}
function fetchLiveModels(key) {
  let raw;
  try {
    raw = host.fetch(JSON.stringify({
      url: "https://api.anthropic.com/v1/models",
      method: "GET",
      timeoutMs: 3e3,
      headers: {
        "x-api-key": key,
        "anthropic-version": "2023-06-01"
      }
    }));
  } catch (_) {
    return null;
  }
  const response = safeJson2(raw);
  if (!response || response.status !== 200 || typeof response.body !== "string") return null;
  const body = safeJson2(response.body);
  const data = body && body.data;
  if (!Array.isArray(data)) return null;
  const out = [];
  for (let i = 0; i < data.length; i++) {
    const item = data[i];
    if (item && typeof item.id === "string" && item.id.trim()) out.push(item);
  }
  return out.length ? out : null;
}
function mapAliasesToLive(aliases, live) {
  const out = [];
  for (let i = 0; i < aliases.length; i++) {
    const alias = aliases[i];
    const family = familyOf(alias);
    const match = family ? bestLiveMatch(live, family) : null;
    const mapped = {
      id: alias.id,
      displayName: match ? withVersion(alias.displayName, versionOf(match)) : alias.displayName
    };
    if (alias.description) mapped.description = alias.description;
    mapped.available = !!match;
    out.push(mapped);
  }
  return out;
}
function familyOf(alias) {
  const hay = (alias.id + " " + alias.displayName).toLowerCase();
  for (let i = 0; i < FAMILIES.length; i++) {
    if (hay.indexOf(FAMILIES[i]) !== -1) return FAMILIES[i];
  }
  return null;
}
function bestLiveMatch(live, family) {
  let best = null;
  let bestKey = -1;
  for (let i = 0; i < live.length; i++) {
    const m = live[i];
    const hay = (String(m.id || "") + " " + liveDisplay(m)).toLowerCase();
    if (hay.indexOf(family) === -1) continue;
    const k = versionKey(versionOf(m));
    if (k >= bestKey) {
      bestKey = k;
      best = m;
    }
  }
  return best;
}
function liveDisplay(m) {
  if (typeof m.display_name === "string" && m.display_name.trim()) return m.display_name.trim();
  if (typeof m.displayName === "string" && m.displayName.trim()) return m.displayName.trim();
  return "";
}
function versionOf(m) {
  const fromName = matchVersion(liveDisplay(m));
  if (fromName) return fromName;
  const id = String(m.id || "");
  const seg = /(\d+)-(\d+)/.exec(id);
  if (seg) return seg[1] + "." + seg[2];
  return matchVersion(id.replace(/-/g, "."));
}
function matchVersion(s) {
  const m = /(\d+(?:\.\d+)?)/.exec(s);
  return m ? m[1] : "";
}
function versionKey(v) {
  if (!v) return 0;
  const parts = v.split(".");
  const major = parseInt(parts[0], 10) || 0;
  const minor = parts.length > 1 ? parseInt(parts[1], 10) || 0 : 0;
  return major * 1e3 + minor;
}
function withVersion(displayName, version) {
  if (!version) return displayName;
  if (/\d+(?:\.\d+)?/.test(displayName)) return displayName.replace(/\d+(?:\.\d+)?/, version);
  return displayName + " " + version;
}
function safeJson2(s) {
  try {
    return JSON.parse(String(s || "null"));
  } catch (_) {
    return null;
  }
}

// plugins/provider-claude/src/classify.ts
var CLASSIFY_THRESHOLD = 0.8;
var RESUME_BUFFER_MS = 60 * 1e3;
function classifyClaudeTabState(snapshot) {
  const screen = String(snapshot && snapshot.screenText || "").replace(/\r\n?/g, "\n");
  if (!screen.trim()) return null;
  if (looksBusy(screen)) return { state: "working", confidence: 0 };
  const activeChoice = activeTerminalChoiceBlock(screen);
  const authConfidence = activeChoice ? scoreAuthPrompt(activeChoice.options.map((option) => option.label).join("\n")) : 0;
  if (authConfidence >= CLASSIFY_THRESHOLD) {
    return {
      state: "waiting_on_auth",
      confidence: authConfidence,
      action: { surface: { kind: "auth_prompt", payload: { reason: "login_detected" } } }
    };
  }
  const rl = scoreRateLimit(screen, host.unixNowMs());
  if (rl.confidence >= CLASSIFY_THRESHOLD && rl.waitKeys && rl.waitKeys.length > 0) {
    const action = { keys: rl.waitKeys };
    const data = {};
    if (rl.resetAtMs !== null) {
      data.resetAt = new Date(rl.resetAtMs).toISOString();
      action.delayedResume = {
        at: new Date(rl.resetAtMs + RESUME_BUFFER_MS).toISOString(),
        text: "continue your work",
        thenKeys: ["enter"]
      };
    } else {
      action.surface = { kind: "rate_limit", payload: { reason: "reset_time_unparsed" } };
    }
    return {
      state: "rate_limited",
      confidence: rl.confidence,
      data: rl.resetAtMs !== null ? data : void 0,
      action
    };
  }
  const cq = scoreClarifyingQuestion(screen);
  if (cq.confidence >= CLASSIFY_THRESHOLD && cq.questions.length > 0) {
    return {
      state: "clarifying_question",
      confidence: cq.confidence,
      data: { questions: cq.questions },
      action: { surface: { kind: "clarifying_question", payload: { questions: cq.questions } } }
    };
  }
  const partial = rl.confidence > cq.confidence ? rl.confidence : cq.confidence;
  if (partial > 0) return { state: "unknown", confidence: partial };
  return { state: "working", confidence: 0 };
}
var CLAUDE_MENU_OPTION_RE = /^[ \t]*([>❯▶➤])?[ \t]*(\d+)[.)][ \t]+(.+?)[ \t]*$/;
function parseClaudeMenuOptions(screen) {
  const lines = String(screen || "").split("\n");
  const options = [];
  for (let i = 0; i < lines.length; i++) {
    const m = CLAUDE_MENU_OPTION_RE.exec(lines[i]);
    if (!m) continue;
    options.push({ highlighted: !!m[1], label: m[3] });
  }
  return options;
}
function navigationKeys(options, targetIdx) {
  let highlightIdx = -1;
  for (let i = 0; i < options.length; i++) {
    if (options[i].highlighted) {
      highlightIdx = i;
      break;
    }
  }
  if (highlightIdx === -1) highlightIdx = 0;
  const keys = [];
  const delta = targetIdx - highlightIdx;
  if (delta > 0) for (let i = 0; i < delta; i++) keys.push("down");
  else if (delta < 0) for (let i = 0; i < -delta; i++) keys.push("up");
  keys.push("enter");
  return keys;
}
function looksBusy(screen) {
  const s = String(screen || "");
  return /\besc to interrupt\b/i.test(s) || /\bctrl\+c to (?:cancel|stop|interrupt|quit)\b/i.test(s) || /\bWorking[….]/.test(s) || /\((?:esc|ctrl\+c) to interrupt\)/i.test(s);
}
function scoreRateLimit(screen, nowMs) {
  const options = parseClaudeMenuOptions(screen);
  let waitIdx = -1;
  let hasUpgradeOption = false;
  for (let i = 0; i < options.length; i++) {
    const label = options[i].label;
    if (/\bupgrade\b/i.test(label)) hasUpgradeOption = true;
    if (waitIdx === -1 && /\b(?:wait|stop and wait)\b/i.test(label) && !/\bupgrade\b/i.test(label)) {
      waitIdx = i;
    }
  }
  const sigWaitUpgradeMenu = waitIdx !== -1 && hasUpgradeOption;
  const sigLimitWord = /\b(?:session|usage|rate|message|weekly)\s+limit\b/i.test(screen) || /\blimit\b[\s\S]{0,40}\breset/i.test(screen);
  const sigResetClock = /\breset[s]?\b[\s\S]{0,40}\d{1,2}(?::\d{2})?\s*(?:am|pm)\b/i.test(screen);
  const sigConfirmFooter = /\bto confirm\b/i.test(screen) && /\bto (?:cancel|exit)\b/i.test(screen);
  let confidence = 0;
  if (sigWaitUpgradeMenu) confidence += 0.5;
  if (sigLimitWord) confidence += 0.25;
  if (sigResetClock) confidence += 0.15;
  if (sigConfirmFooter) confidence += 0.1;
  if (confidence > 1) confidence = 1;
  const waitKeys = sigWaitUpgradeMenu ? navigationKeys(options, waitIdx) : null;
  const resetAtMs = parseResetClock(screen, nowMs);
  return { confidence, waitKeys, resetAtMs };
}
function parseResetClock(screen, nowMs) {
  const m = /\breset[s]?\b[^\n]{0,40}?(\d{1,2}(?::\d{2})?\s*(?:am|pm))\b/i.exec(String(screen || ""));
  if (!m) return null;
  return usageParseTodayClock(m[1], nowMs);
}
function scoreClarifyingQuestion(screen) {
  const block = activeClarifyingPromptBlock(screen);
  const options = block?.options ?? [];
  const questions = block?.questions ?? [];
  const sigMenu = options.length >= 2;
  const sigQuestion = questions.length > 0;
  const sigConfirmFooter = !!block;
  let confidence = 0;
  if (sigMenu) confidence += 0.35;
  if (sigQuestion) confidence += 0.3;
  if (sigConfirmFooter) confidence += 0.35;
  if (confidence > 1) confidence = 1;
  return { confidence, questions };
}
function activeTerminalChoiceBlock(screen) {
  const lines = String(screen || "").split("\n");
  for (let footerIdx = lines.length - 1; footerIdx >= 0; footerIdx--) {
    if (!isActiveChoiceFooter(lines[footerIdx])) continue;
    if (lastMeaningfulLineIndex(lines) > footerIdx) return null;
    const options = contiguousOptionsAbove(lines, footerIdx);
    if (options.length >= 2 && options.some((option) => option.highlighted)) {
      return { footerIdx, options };
    }
  }
  return null;
}
function activeClarifyingPromptBlock(screen) {
  const lines = String(screen || "").split("\n");
  const choice = activeTerminalChoiceBlock(screen);
  if (!choice || !isClarifyingConfirmFooter(lines[choice.footerIdx])) return null;
  const questions = questionsAbove(lines, choice.footerIdx);
  if (questions.length === 0) {
    const prompt = promptAboveOptions(lines, choice.footerIdx);
    if (prompt) questions.push(prompt);
  }
  return questions.length > 0 ? { questions, options: choice.options } : null;
}
function isActiveChoiceFooter(line) {
  return /\b(?:press\s+)?enter\s+to\b/i.test(line);
}
function isClarifyingConfirmFooter(line) {
  return /\bto (?:confirm|submit|select)\b/i.test(line);
}
function lastMeaningfulLineIndex(lines) {
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line) continue;
    if (isDividerLine(line)) continue;
    return i;
  }
  return -1;
}
function isDividerLine(line) {
  const stripped = line.replace(/\s/g, "");
  return stripped.length >= 8 && /^[─━═-]+$/.test(stripped);
}
function contiguousOptionsAbove(lines, footerIdx) {
  const options = [];
  for (let i = footerIdx - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line) {
      if (options.length > 0) break;
      continue;
    }
    const m = CLAUDE_MENU_OPTION_RE.exec(lines[i]);
    if (!m) {
      if (options.length > 0) break;
      continue;
    }
    options.push({ highlighted: !!m[1], label: m[3] });
  }
  return options.reverse();
}
function promptAboveOptions(lines, footerIdx) {
  for (let i = footerIdx - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (!line || CLAUDE_MENU_OPTION_RE.test(lines[i]) || isDividerLine(line)) continue;
    return line;
  }
  return null;
}
function questionsAbove(lines, footerIdx) {
  const out = [];
  for (let i = footerIdx - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (line.length > 1 && line.charAt(line.length - 1) === "?") out.push(line);
    if (out.length > 0 && CLAUDE_MENU_OPTION_RE.test(line)) continue;
    if (out.length > 0 && !line) break;
  }
  return out.reverse();
}
function scoreAuthPrompt(screen) {
  const s = String(screen || "");
  let score = 0;
  if (/\bsign[ -]?in\b/i.test(s)) score += 0.5;
  if (/\blog[ -]?in\b/i.test(s)) score += 0.5;
  if (/\bauthenticat(?:e|ion|ing)\b/i.test(s)) score += 0.5;
  if (/\bo\s?auth\b/i.test(s)) score += 0.6;
  if (/\bapi key\b/i.test(s)) score += 0.6;
  if (/paste\b[^\n]*\b(token|key|code|credential)/i.test(s)) score += 0.6;
  if (/\bauthorization code\b/i.test(s)) score += 0.6;
  if (/enter your\b[^\n]*\b(api key|token|credential)/i.test(s)) score += 0.6;
  return score > 1 ? 1 : score;
}

// plugins/provider-claude/src/prompts.ts
var WIZARD_SUBMIT_VALUE = 9e3;
var WIZARD_MULTI_MASK_BASE = 1e4;
function parseClaudeWizardPrompt(text) {
  const raw = String(text || "");
  if (raw.indexOf("Submit") === -1 || raw.indexOf("Enter to select") === -1) return null;
  const tabLine = findWizardTabLine(raw);
  if (!tabLine) return null;
  const sections = parseWizardSections(tabLine);
  if (sections.length < 2) return null;
  const body = wizardBody(raw, tabLine);
  const options = parseWizardOptions(body);
  if (options.length < 2) return null;
  const question = parseWizardQuestion(body) || "Choose an option";
  const multiSelect = options.some((opt) => opt.selected) || /\[[ xX]\]|☐|☑|☒/.test(body);
  return {
    question,
    options: options.map((opt) => [opt.label, opt.value]),
    sections,
    multiSelect,
    submitAvailable: tabLine.indexOf("Submit") !== -1,
    selectedValues: options.filter((opt) => opt.selected).map((opt) => opt.value)
  };
}
function findWizardTabLine(text) {
  const lines = String(text || "").split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i];
    if (line.indexOf("Submit") !== -1 && (line.indexOf("\u2610") !== -1 || line.indexOf("\u2714") !== -1)) {
      return line;
    }
  }
  const m = /←[\s\S]{0,160}Submit\s*→/.exec(text);
  return m ? m[0] : null;
}
function parseWizardSections(tabLine) {
  const sections = [];
  const re = /([☐☑☒✔])\s*([^☐☑☒✔←→]+?)(?=[☐☑☒✔]|→|$)/g;
  let m;
  while ((m = re.exec(tabLine)) !== null) {
    const label = m[2].trim();
    if (!label || /^submit$/i.test(label)) continue;
    sections.push({
      label,
      completed: m[1] === "\u2714" || m[1] === "\u2611" || m[1] === "\u2612"
    });
  }
  const active = sections.findIndex((s) => !s.completed);
  if (active >= 0) sections[active].active = true;
  return sections;
}
function wizardBody(text, knownTabLine) {
  const tabLine = knownTabLine || findWizardTabLine(text);
  if (!tabLine) return text;
  return text.slice(text.indexOf(tabLine) + tabLine.length);
}
function parseWizardQuestion(body) {
  const beforeOptions = body.split(/(?:❯\s*)?\d+\./)[0] || "";
  return beforeOptions.replace(/^[\s←→☐☑☒✔A-Za-z]*Submit\s*→?/, "").replace(/─+/g, " ").replace(/\s+/g, " ").trim();
}
function parseWizardOptions(body) {
  const checkbox = parseCheckboxOptions(body);
  if (checkbox.length >= 2) return checkbox;
  return parseNumberedOptions(body);
}
function parseNumberedOptions(text) {
  const options = [];
  const re = /^\s*[❯>]?\s*(\d+)\.\s*([^\r\n]*)/gm;
  let m;
  while ((m = re.exec(text)) !== null) {
    const label = cleanOptionLabel(m[2]);
    if (!label) continue;
    options.push({
      label,
      value: m[1],
      highlighted: m[0].indexOf("\u276F") !== -1 || m[0].indexOf(">") !== -1,
      selected: false
    });
  }
  return options;
}
function parseCheckboxOptions(text) {
  const options = [];
  const re = /^\s*[❯>]?\s*((?:\[[ xX]\])|☐|☑|☒|✔)\s*([^\r\n]*)/gm;
  let m;
  while ((m = re.exec(text)) !== null) {
    const label = cleanOptionLabel(m[2]);
    if (!label) continue;
    const mark = m[1];
    options.push({
      label,
      value: String(options.length + 1),
      highlighted: m[0].indexOf("\u276F") !== -1 || m[0].indexOf(">") !== -1,
      selected: mark === "\u2611" || mark === "\u2612" || mark === "\u2714" || /\[[xX]\]/.test(mark)
    });
  }
  return options;
}
function cleanOptionLabel(value) {
  return String(value || "").replace(/─+/g, " ").replace(/\bChat\s*about\s*this\b[\s\S]*$/i, "").replace(/\s+/g, " ").trim();
}
function parseClaudeNumberedPrompt(text) {
  const raw = String(text || "");
  if (!/(Esc\s*to\s*cancel|Enter\s*to\s*(?:confirm|select)|Tab\s*to\s*amend|❯)/i.test(raw)) return null;
  const options = parseNumberedOptions(raw);
  if (options.length < 2) return null;
  const firstOption = raw.search(/(?:^|[\s❯>])\d+\.\s*/);
  const before = firstOption >= 0 ? raw.slice(0, firstOption) : raw;
  const question = findQuestion(before) || "Choose an option";
  return {
    question,
    options: options.map((opt) => [opt.label, opt.value])
  };
}
function parseClaudeLetteredPrompt(text) {
  const lines = String(text || "").replace(/\r\n?/g, "\n").split("\n");
  const optionStarts = [];
  const optionRe = /^\s*(?:[-*]\s*)?([A-Z])[.)]\s+(.+?)\s*$/;
  for (let i = 0; i < lines.length; i++) {
    const m = optionRe.exec(lines[i]);
    if (!m || !/[A-C]/.test(m[1])) continue;
    optionStarts.push({ index: i, letter: m[1], label: m[2] });
  }
  if (optionStarts.length < 3) return null;
  const options = [];
  for (let i = 0; i < optionStarts.length; i++) {
    const start = optionStarts[i];
    const end = i + 1 < optionStarts.length ? optionStarts[i + 1].index : lines.length;
    const parts = [start.label];
    for (let j = start.index + 1; j < end; j++) {
      const t = lines[j].trim();
      if (!t) break;
      if (isClaudeFooterLine(t)) break;
      if (/^Step\s+\d+\s+of\s+\d+\b/i.test(t)) break;
      if (t.endsWith("?")) break;
      parts.push(t);
    }
    options.push({
      label: cleanOptionLabel(parts.join(" ")),
      value: start.letter,
      highlighted: false,
      selected: false
    });
  }
  const groups = groupLetteredOptions(lines, optionStarts);
  const question = groups.length > 1 ? findCompoundTitle(lines, groups[0].questionIndex) || groups[0].question || "Choose an option" : groups[0]?.question || findQuestion(lines.slice(0, optionStarts[0].index).join("\n")) || "Choose an option";
  const sections = groups.length > 1 ? groups.map((group, i) => ({
    label: group.title || "Step " + String(i + 1),
    active: i === 0,
    completed: false
  })) : void 0;
  return {
    question,
    options: options.map((opt) => [opt.label, opt.value]),
    sections,
    multiSelect: false,
    submitAvailable: false
  };
}
function isClaudeFooterLine(line) {
  return line.indexOf("Baked for") !== -1 || line.indexOf("Cogitated for") !== -1 || line.indexOf("Spelunking") !== -1 || line.indexOf("auto mode on") !== -1 || line.indexOf("\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500") !== -1 || line === "\u276F";
}
function groupLetteredOptions(lines, starts) {
  const groups = [];
  for (let i = 0; i < starts.length; i++) {
    if (starts[i].letter !== "A") continue;
    const questionIndex = nearestQuestionIndex(lines, starts[i].index);
    const title = nearestSectionTitle(lines, questionIndex);
    groups.push({
      question: questionIndex >= 0 ? cleanClaudeLine(lines[questionIndex]) : "",
      questionIndex,
      title
    });
  }
  return groups;
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
    const t = cleanClaudeLine(lines[i]);
    if (!t) continue;
    const step = /^Step\s+\d+\s+of\s+\d+\s+[—-]\s+(.+)$/i.exec(t);
    if (step && step[1]) return step[1].trim();
    if (!t.endsWith("?") && !/^[❯>]/.test(t)) return t;
  }
  return null;
}
function findCompoundTitle(lines, beforeQuestionIndex) {
  if (beforeQuestionIndex < 0) return null;
  for (let i = beforeQuestionIndex - 1; i >= Math.max(0, beforeQuestionIndex - 8); i--) {
    const t = cleanClaudeLine(lines[i]);
    if (!t) continue;
    if (/^Step\s+\d+\s+of\s+\d+\b/i.test(t)) continue;
    if (t.endsWith("?") || /^[❯>]/.test(t)) continue;
    return t;
  }
  return null;
}
function cleanClaudeLine(line) {
  return String(line || "").trim().replace(/^⏺\s*/, "").trim();
}
function findQuestion(text) {
  const lines = String(text || "").split("\n").map((line) => line.trim()).filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].endsWith("?")) return lines[i];
  }
  return lines.length > 0 ? lines[lines.length - 1] : "";
}
function buildClaudePromptResponse(optionIndex, screen) {
  if (optionIndex === WIZARD_SUBMIT_VALUE - 1) return buildWizardSubmitResponse(screen);
  if (optionIndex >= WIZARD_MULTI_MASK_BASE - 1) {
    return buildWizardMultiSelectResponse(optionIndex - (WIZARD_MULTI_MASK_BASE - 1), screen);
  }
  const steps = moveToOption(optionIndex, screen);
  steps.push("DQ==");
  if (parseClaudeWizardPrompt(screen) && !parseClaudeWizardPrompt(screen)?.multiSelect) steps.push("CQ==");
  return steps;
}
function moveToOption(targetIndex, screen) {
  const current = currentHighlightedOptionIndex(screen);
  const delta = targetIndex - current;
  const steps = [];
  const key = delta >= 0 ? "ag==" : "aw==";
  for (let i = 0; i < Math.abs(delta); i++) steps.push(key);
  return steps;
}
function currentHighlightedOptionIndex(screen) {
  const body = wizardBody(screen);
  const numbered = parseNumberedOptions(body);
  const options = numbered.length >= 2 ? numbered : parseWizardOptions(body);
  const idx = options.findIndex((opt) => opt.highlighted);
  return idx >= 0 ? idx : 0;
}
function buildWizardMultiSelectResponse(mask, screen) {
  const options = parseWizardOptions(wizardBody(screen));
  const steps = [];
  let current = currentHighlightedOptionIndex(screen);
  for (let i = 0; i < options.length; i++) {
    const desired = (mask & 1 << i) !== 0;
    if (options[i].selected === desired) continue;
    const delta = i - current;
    const key = delta >= 0 ? "ag==" : "aw==";
    for (let j = 0; j < Math.abs(delta); j++) steps.push(key);
    steps.push("IA==");
    current = i;
  }
  steps.push("CQ==");
  return steps;
}
function buildWizardSubmitResponse(screen) {
  const parsed = parseClaudeWizardPrompt(screen);
  const steps = [];
  const active = parsed?.sections?.findIndex((section) => section.active) ?? -1;
  const count = parsed?.sections?.length ?? 0;
  const moves = active >= 0 && count > active ? count - active : 1;
  for (let i = 0; i < moves; i++) steps.push("G1tD");
  steps.push("DQ==");
  return steps;
}

// plugins/provider-claude/src/plugin.ts
var TITLE_RE = /claude/i;
var BRAILLE_LO = 10241;
var BRAILLE_HI = 10495;
var OUTPUT_FINGERPRINTS = [
  "Claude Code",
  "Do you want to proceed?",
  "may I proceed",
  "Would you like me to",
  "Yes, and don't ask again",
  "Enter to confirm",
  "Cogitated for",
  "Brewed for",
  "Reasoned for"
];
var NOISE_FRAGMENTS = [
  "bypass permissions on",
  "shift+tab to cycle",
  "Update available! Run:",
  "brew upgrade claude-code",
  "esc to interrupt",
  "? for shortcuts",
  "ctrl+g to edit",
  "Monthly spend limit",
  "hold Space"
];
var FILE_TASK_PROMPT = "Execute the CodeTerm task provided in the appended system prompt. Follow its reporting instructions exactly.";
var IDLE_WAKEUP_START_PROMPT = "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";
function starterPrompt(prompt, hostText) {
  if (prompt === "no_starter") return void 0;
  if (prompt === "idle_wakeup") return IDLE_WAKEUP_START_PROMPT;
  if (prompt === "team_bootstrap") {
    if (!hostText) throw new Error("team_bootstrap requires host starterPromptText");
    return hostText;
  }
  return FILE_TASK_PROMPT;
}
function xmlTagBody(content, tag) {
  const open = `<${tag}>`;
  const close = `</${tag}>`;
  const start = content.indexOf(open);
  if (start < 0) return null;
  const bodyStart = start + open.length;
  const end = content.indexOf(close, bodyStart);
  if (end < 0) return null;
  return content.slice(bodyStart, end).trim();
}
function taskNotificationStatus(content) {
  const trimmed = content.trim();
  const start = trimmed.indexOf("<task-notification>");
  const end = trimmed.indexOf("</task-notification>", start);
  if (start < 0 || end < 0) {
    return null;
  }
  const block = trimmed.slice(start, end + "</task-notification>".length);
  const status = xmlTagBody(block, "status");
  return status ? `Task ${status}` : "Task notification";
}
function isCompactSummaryRow(entry, content) {
  if (entry.isCompactSummary === true) return true;
  return entry.isVisibleInTranscriptOnly === true && content.trimStart().startsWith("This session is being continued from a previous conversation");
}
var TUI_FRAGMENTS = [
  "? for shortcuts",
  "for shortcuts",
  "ctrl+l",
  "esc to interrupt",
  "bypass permissions",
  "shift+tab to cycle"
];
var SESSION_EXPLICIT_RE = /session(?:\s+id)?\s*[:=]\s*([a-z]+-[a-z]+-\d{2,6})\b/i;
var SESSION_BARE_RE = /(?:^|\n)\s*([a-z]{4,10}-[a-z]{4,10}-\d{4})\s*(?:$|\n)/m;
function sessionProjectPath(head) {
  const lines = String(head || "").split("\n", 64);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]?.trim();
    if (!line) continue;
    try {
      const entry = JSON.parse(line);
      if (typeof entry.cwd === "string" && entry.cwd.trim()) return entry.cwd;
    } catch {
    }
  }
  return null;
}
function isBrailleSpinner(ch) {
  if (!ch) return false;
  const code = ch.charCodeAt(0);
  return code >= BRAILLE_LO && code <= BRAILLE_HI || ch === "\u2733" || ch === "\u273B";
}
var plugin = {
  starterPromptText(prompt) {
    return prompt === "team_bootstrap" ? void 0 : starterPrompt(prompt);
  },
  detectFromTitle(title) {
    const t = String(title || "");
    if (TITLE_RE.test(t)) return true;
    const first = t.charAt(0);
    return isBrailleSpinner(first) && t.length > 2;
  },
  detectFromOutput(text) {
    const t = String(text || "");
    for (let i = 0; i < OUTPUT_FINGERPRINTS.length; i++) {
      if (t.indexOf(OUTPUT_FINGERPRINTS[i]) !== -1) return true;
    }
    return false;
  },
  screenHasTui(screen) {
    const s = String(screen || "");
    const lower = s.toLowerCase();
    for (let i = 0; i < TUI_FRAGMENTS.length; i++) {
      if (lower.indexOf(TUI_FRAGMENTS[i]) !== -1) return true;
    }
    return s.indexOf("\u259B") !== -1 || s.indexOf("\u259C") !== -1;
  },
  isOutputNoise(text) {
    const trimmed = String(text || "").trim();
    if (!trimmed) return true;
    for (let i = 0; i < NOISE_FRAGMENTS.length; i++) {
      if (trimmed.indexOf(NOISE_FRAGMENTS[i]) !== -1) return true;
    }
    for (let j = 0; j < trimmed.length; j++) {
      const c = trimmed[j];
      if (c !== "\u2500" && c !== "\u2501" && c !== "\u2550" && c !== " " && c !== "	" && c !== "\n" && c !== "\r") {
        return false;
      }
    }
    return true;
  },
  hasEventMarkers(chunk) {
    const c = String(chunk || "");
    if (c.indexOf("\u276F") !== -1) return true;
    if (c.indexOf("Enter to select") !== -1) return true;
    if (c.indexOf("Submit") !== -1 && (c.indexOf("\u2610") !== -1 || c.indexOf("\u2714") !== -1)) return true;
    if (c.indexOf("proceed") !== -1) return true;
    if (c.indexOf("Esc to cancel") !== -1) return true;
    if (c.indexOf("Tab to amend") !== -1) return true;
    if (c.indexOf("1.") !== -1 && c.indexOf("2.") !== -1) return true;
    for (let i = 0; i < 10; i++) {
      if (c.indexOf(" for " + i) !== -1) return true;
    }
    return false;
  },
  extractSessionName(screen) {
    const s = String(screen || "");
    let m = SESSION_EXPLICIT_RE.exec(s);
    if (m && m[1]) return m[1];
    m = SESSION_BARE_RE.exec(s);
    if (m && m[1]) return m[1];
    return null;
  },
  describeSession(_filePath, head) {
    const projectPath = sessionProjectPath(head ?? "");
    return projectPath ? { projectPath } : null;
  },
  detectEvents(text) {
    const t = String(text || "");
    const events = [];
    if (parseClaudeWizardPrompt(t)) events.push("permission_request");
    const numLines = (t.match(/^\s*\d+\.\s+/gm) || []).length;
    const hasEscFooter = t.indexOf("Esc to cancel") !== -1 || t.indexOf("Tab to amend") !== -1;
    const hasSelector = t.indexOf("\u276F") !== -1;
    if (numLines >= 2 && (hasEscFooter || hasSelector)) {
      events.push("permission_request");
    }
    const lines = t.split("\n");
    const start = Math.max(0, lines.length - 10);
    for (let i = start; i < lines.length; i++) {
      const line = lines[i].trim();
      const pos = line.indexOf(" for ");
      if (pos < 1) continue;
      const word = line.substring(0, pos);
      const after = line.substring(pos + 5);
      if (!/^[A-Z][a-zA-Z]*$/.test(word)) continue;
      if (/^\d+[smh]/.test(after) || /^\d/.test(after) && /[smh]/.test(after)) {
        events.push("task_complete");
        break;
      }
    }
    return events;
  },
  parsePrompt(text) {
    return parseClaudeWizardPrompt(text) || parseClaudeNumberedPrompt(text) || parseClaudeLetteredPrompt(text);
  },
  buildPromptResponse(optionIndex, screen) {
    return buildClaudePromptResponse(optionIndex, String(screen || ""));
  },
  directoryTrustPromptResponse(prompt, screen) {
    const question = String(prompt.question || "").toLowerCase();
    const first = prompt.options[0];
    const firstLabel = Array.isArray(first) ? first[0] : first.label;
    const isTrustGate = question.includes("trust") && question.includes("project");
    if (!isTrustGate || !firstLabel || !/^yes\b.*trust/i.test(firstLabel)) return null;
    return buildClaudePromptResponse(0, String(screen || ""));
  },
  // Launch command parity for native behavior across OS-specific execution modes.
  buildLaunchCommand(params) {
    const p = params || {};
    const win = host.platform() === "windows";
    const unset = win ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; $OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = [System.Text.Encoding]::UTF8; $PSNativeCommandArgumentPassing = 'Standard';" : "unset CLAUDECODE &&";
    const parts = [unset];
    parts.push("claude");
    if (p.skipPermissions) parts.push("--dangerously-skip-permissions");
    if (p.agentName) {
      parts.push("--agent");
      parts.push(host.shell.quoteFor(p.agentName, host.platform()));
    }
    if (p.sessionId) {
      parts.push("--session-id");
      parts.push(host.shell.quoteFor(p.sessionId, host.platform()));
    }
    if (p.args && p.args.length > 0) {
      for (let i = 0; i < p.args.length; i++) parts.push(host.shell.quoteFor(String(p.args[i]), host.platform()));
    }
    if (p.task) {
      const visiblePath = writeTaskTempFile(p.task);
      parts.push("--append-system-prompt-file");
      parts.push(host.shell.quoteFor(String(visiblePath), host.platform()));
      const prompt = starterPrompt(p.starterPrompt, p.starterPromptText);
      if (prompt) {
        parts.push("--");
        parts.push(host.shell.quoteFor(prompt, host.platform()));
      }
    }
    return parts.join(" ");
  },
  buildResumeCommand(sessionId, skipPermissions) {
    const win = host.platform() === "windows";
    const unset = win ? "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; $OutputEncoding = [Console]::OutputEncoding = [Console]::InputEncoding = [System.Text.Encoding]::UTF8; $PSNativeCommandArgumentPassing = 'Standard';" : "unset CLAUDECODE &&";
    const parts = [unset, "claude"];
    if (skipPermissions) parts.push("--dangerously-skip-permissions");
    parts.push("--resume");
    parts.push(host.shell.quoteFor(sessionId, host.platform()));
    return parts.join(" ");
  },
  // Tail session JSONL to find latest model (`message.model` or `Set model to ...`).
  detectSessionModel(cwd, sessionId) {
    const path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    const contents = host.fs.readFile(path);
    if (!contents) return null;
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      const line = lines[i];
      if (!line) continue;
      let v;
      try {
        v = JSON.parse(line);
      } catch (_) {
        continue;
      }
      if (!v || !v.message) continue;
      const raw = contentToText(v.message.content);
      const fam = extractSetModelFamily(raw);
      if (fam) return fam;
      const m = v.message.model;
      if (typeof m === "string" && m && m !== "<synthetic>") {
        return familyFromModelId(m);
      }
    }
    return null;
  },
  detectSessionReasoningEffort(cwd, sessionId) {
    const path = claudeSessionFilePath(cwd, sessionId);
    if (!path || !host.fs.fileExists(path)) return null;
    const contents = host.fs.readFileTail(path, 4 * 1024 * 1024);
    if (!contents) return null;
    const supported = { low: true, medium: true, high: true, xhigh: true, max: true };
    const lines = contents.split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      if (!lines[i]) continue;
      let value;
      try {
        value = JSON.parse(lines[i]);
      } catch (_) {
        continue;
      }
      const message = value.message && typeof value.message === "object" ? value.message : null;
      const metadata = message?.metadata && typeof message.metadata === "object" ? message.metadata : null;
      const candidates = [value.effort, value.reasoning_effort, value.reasoningEffort, message?.effort, message?.reasoning_effort, metadata?.effort];
      for (let j = 0; j < candidates.length; j++) {
        const candidate = candidates[j];
        const effort = typeof candidate === "string" ? candidate.trim() : "";
        if (supported[effort]) return effort;
      }
    }
    return null;
  },
  // Discovery is genuine-only: null (no keys / failed fetch) keeps the host on
  // the registered manifest seed and must never be persisted as a live snapshot.
  discoverModels() {
    return discoverAnthropicModels();
  },
  // Parse the `/usage` TUI screen into a UsageSnapshot; null when nothing recognizable was found.
  parseUsage(rawText, nowMs) {
    const raw = String(rawText || "");
    if (!raw.trim()) return null;
    const fetched = parseFetchedClaudeUsage(raw, nowMs);
    if (fetched !== void 0) return fetched;
    const text = usageNormalize(raw);
    const session = usageParseBlock(text, "Current session", false, nowMs);
    const weekly = usageParseBlock(text, "Current week\\s*\\(\\s*all\\s*models\\s*\\)", true, nowMs);
    if (session.pct === null && weekly.pct === null) return null;
    if (!usageBlockIsComplete(session) || !usageBlockIsComplete(weekly)) return null;
    return {
      provider: "claude",
      account_id: null,
      captured_at_ms: nowMs,
      session_pct: session.pct,
      session_resets_at_ms: session.resetsMs,
      weekly_pct: weekly.pct,
      weekly_resets_at_ms: weekly.resetsMs,
      raw_text: raw
    };
  },
  fetchUsage() {
    return fetchClaudeUsage(host);
  },
  // Duplicated by design across provider bundles (QuickJS, no imports) — parity guarded by provider-helper-parity.test.ts. Change both or the test fails.
  // Parse `user`/`assistant` lines and map content blocks to SessionMessage.
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
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!isValidClaudeSessionMessage(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionMessages: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const msgType = v && v.type || "";
      if (msgType !== "user" && msgType !== "assistant") continue;
      const isMeta = !!(v && v.isMeta);
      const msg = v.message;
      if (!msg) continue;
      const content = msg.content;
      let text2 = "";
      if (typeof content === "string") {
        text2 = content;
      } else if (Array.isArray(content)) {
        const chunks = [];
        for (let j = 0; j < content.length; j++) {
          const b = content[j];
          if (b && b.type === "text" && typeof b.text === "string") {
            chunks.push(b.text);
          }
        }
        text2 = chunks.join(" ");
      } else {
        continue;
      }
      if (!text2 || !text2.trim()) continue;
      let ts = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      out.push({ role: msgType, text: text2, ts, isMeta });
    }
    return out;
  },
  // Rust provides complete lines; plugin parses JSON into RichMessage[]. Byte offsets stay in Rust.
  parseSessionDelta(chunk) {
    const out = [];
    const lines = String(chunk || "").split("\n");
    let hadInvalid = false;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (!line) continue;
      let v;
      try {
        v = JSON.parse(line);
      } catch (_) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      if (!validateEntry(v)) {
        if (!hadInvalid) {
          if (typeof console !== "undefined") console.warn("[claude-plugin] parseSessionDelta: skipped invalid JSONL entry");
          hadInvalid = true;
        }
        continue;
      }
      const t = v && v.type;
      const blocks = [];
      let role;
      let sourceSubtype = typeof v.subtype === "string" ? v.subtype : null;
      if (t === "system") {
        role = "system";
        const sysText = typeof v.content === "string" ? v.content : Array.isArray(v.content) ? v.content.filter((c) => c && c.type === "text").map((c) => c.text || "").join("\n") : "";
        const sdata = { text: sysText };
        if (typeof v.subtype === "string") sdata.subtype = v.subtype;
        if (typeof v.level === "string") sdata.level = v.level;
        blocks.push({ kind: "text", data: sdata });
      } else if (t === "user" || t === "assistant") {
        role = t;
        const msg = v.message;
        if (!msg) continue;
        const content = msg.content;
        if (typeof content === "string") {
          const taskStatus = t === "user" ? taskNotificationStatus(content) : null;
          if (taskStatus) {
            role = "status";
            sourceSubtype = "task_notification";
            blocks.push({ kind: "text", data: { text: taskStatus, subtype: sourceSubtype } });
          } else if (t === "user" && isCompactSummaryRow(v, content)) {
            role = "system";
            sourceSubtype = "compact_summary";
            blocks.push({ kind: "text", data: { text: content, subtype: sourceSubtype } });
          } else if (content) {
            blocks.push({ kind: "text", data: { text: content } });
          }
        } else if (Array.isArray(content)) {
          for (let j = 0; j < content.length; j++) {
            const b = content[j];
            if (!b || typeof b.type !== "string") continue;
            const data = {};
            for (const k in b) {
              if (Object.prototype.hasOwnProperty.call(b, k) && k !== "type") {
                data[k] = b[k];
              }
            }
            if (b.type === "tool_use") {
              const canonical = canonicalToolFields(
                typeof data.name === "string" ? data.name : "",
                data.input
              );
              data.tool_kind = canonical.kind;
              data.tool_summary = canonical.summary;
              data.tool_detail = canonical.detail;
              data.tool_edits = canonical.edits;
            }
            blocks.push({ kind: b.type, data });
          }
        } else {
          continue;
        }
      } else if (t === "attachment") {
        const att = v.attachment;
        if (!att || att.type !== "queued_command" || typeof att.prompt !== "string" || !att.prompt) {
          continue;
        }
        const taskStatus = att.commandMode === "task-notification" ? taskNotificationStatus(att.prompt) ?? "Task notification" : taskNotificationStatus(att.prompt);
        if (taskStatus) {
          role = "status";
          sourceSubtype = "task_notification";
          blocks.push({ kind: "text", data: { text: taskStatus, subtype: sourceSubtype } });
        } else {
          role = "user";
          blocks.push({ kind: "text", data: { text: att.prompt } });
        }
      } else {
        continue;
      }
      if (!blocks.length) continue;
      let ts = null;
      if (typeof v.timestamp === "string") {
        const d = Date.parse(v.timestamp);
        if (!isNaN(d)) ts = Math.floor(d / 1e3);
      }
      out.push({
        role,
        blocks,
        ts,
        uuid: typeof v.uuid === "string" ? v.uuid : null,
        isMeta: v.isMeta === true ? true : null,
        sourceType: typeof v.type === "string" ? v.type : null,
        sourceSubtype,
        level: typeof v.level === "string" ? v.level : null,
        isCompactSummary: v.isCompactSummary === true ? true : null,
        isVisibleInTranscriptOnly: v.isVisibleInTranscriptOnly === true ? true : null,
        timestamp: typeof v.timestamp === "string" ? v.timestamp : null
      });
    }
    return { messages: out };
  },
  // Keep JSONL path encoding aligned with `claudeSessionFilePath`.
  sessionJsonlPath(_cwd, sessionId) {
    if (!sessionId) return null;
    return claudeSessionFilePath(_cwd, sessionId);
  },
  // Write bridge rule to `~/.claude/rules/codeterm.md` via host APIs.
  installBridgeRule(payload) {
    const m = manifest();
    const rulePath = m && m.rule && m.rule.path || "~/.claude/rules/codeterm.md";
    const expanded = host.expandHome(rulePath);
    if (!expanded) return { ok: false, error: "could not expand " + rulePath };
    const ok = host.writeFile(expanded, payload);
    if (ok) {
      host.log("info", "[claude-plugin] bridge rule installed to " + expanded);
      return { ok: true };
    }
    return { ok: false, error: "writeFile failed for " + expanded };
  },
  uninstallBridgeRule() {
    const m = manifest();
    const rulePath = m && m.rule && m.rule.path || "~/.claude/rules/codeterm.md";
    const expanded = host.expandHome(rulePath);
    if (!expanded) return { ok: true };
    if (!host.fileExists(expanded)) return { ok: true };
    const ok = host.removeFile(expanded);
    if (ok) {
      host.log("info", "[claude-plugin] bridge rule removed from " + expanded);
      return { ok: true };
    }
    return { ok: false, error: "removeFile failed for " + expanded };
  },
  // Manage Claude settings hooks for Notification, Stop, PreCompact, SessionStart.
  // Hooks use a versioned source marker and runtime callback identity.
  checkIntegration() {
    const m = manifest();
    const path = host.expandHome(m && m.integration && m.integration.settingsPath || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) {
      return {
        settings_exists: false,
        hooks_installed: false,
        notification_hook: false,
        stop_hook: false,
        precompact_hook: false,
        session_start_hook: false,
        target: path || "~/.claude/settings.json",
        backup_exists: !!path && host.fileExists(path + ".codeterm-backup"),
        proposed_changes: ["Notification", "Stop", "SessionStart"]
      };
    }
    const json = host.fs.readJson(path);
    if (!json) {
      return { settings_exists: true, hooks_installed: false };
    }
    function present(key) {
      const arr = json.hooks && json.hooks[key];
      if (!Array.isArray(arr)) return false;
      for (let i = 0; i < arr.length; i++) {
        const entry = arr[i];
        const hooks = entry && entry.hooks;
        if (!Array.isArray(hooks)) continue;
        for (let j = 0; j < hooks.length; j++) {
          const hook = hooks[j];
          if (hook && hook._source === CODETERM_HOOK_MARKER && typeof hook.command === "string" && hook.command.indexOf("CODETERM_TAB_ID") !== -1 && hook.command.indexOf("CODETERM_API_URL") !== -1 && hook.command.indexOf("CODETERM_API_TOKEN") !== -1 && hook.command.indexOf("CODETERM_API_PORT") === -1 && hook.command.indexOf("CODETERM_PANE_ID") === -1 && hook.command.indexOf("/api/panes/") === -1) return true;
        }
      }
      return false;
    }
    function sessionStartCurrent() {
      const arr = json.hooks && json.hooks.SessionStart;
      if (!Array.isArray(arr)) return false;
      for (let i = 0; i < arr.length; i++) {
        const hooks = arr[i] && arr[i].hooks;
        if (!Array.isArray(hooks)) continue;
        for (let j = 0; j < hooks.length; j++) {
          const h = hooks[j];
          if (h && h._source === CODETERM_HOOK_MARKER && typeof h.command === "string" && h.command.indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1 && h.command.indexOf("CODETERM_TAB_ID") !== -1 && h.command.indexOf("CODETERM_API_URL") !== -1 && h.command.indexOf("CODETERM_API_TOKEN") !== -1 && h.command.indexOf("CODETERM_API_PORT") === -1 && h.command.indexOf("CODETERM_PANE_ID") === -1 && h.command.indexOf("/api/panes/") === -1) return true;
        }
      }
      return false;
    }
    const n = present("Notification");
    const s = present("Stop");
    const pc = present("PreCompact");
    const ss = sessionStartCurrent();
    return {
      settings_exists: true,
      hooks_installed: n && s && ss,
      notification_hook: n,
      stop_hook: s,
      precompact_hook: pc,
      session_start_hook: ss,
      target: path,
      backup_exists: host.fileExists(path + ".codeterm-backup"),
      proposed_changes: ["Notification", "Stop", "SessionStart"]
    };
  },
  installIntegration(_apiPort, options) {
    if (!options || options.consent !== true) {
      return { ok: false, error: "provider integration installation requires explicit local-user consent" };
    }
    const m = manifest();
    const path = host.expandHome(m && m.integration && m.integration.settingsPath || "~/.claude/settings.json");
    if (!path) return { ok: false, error: "could not expand settingsPath" };
    const existed = host.fileExists(path);
    const originalValue = existed ? host.fs.readFile(path) : null;
    if (existed && typeof originalValue !== "string") {
      return { ok: false, error: "could not read existing settings JSON at " + path };
    }
    const original = typeof originalValue === "string" ? originalValue : "";
    let parsed = {};
    if (existed) {
      try {
        parsed = JSON.parse(original);
      } catch (_) {
        return { ok: false, error: "existing settings JSON is malformed at " + path };
      }
    }
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { ok: false, error: "existing settings JSON must contain an object at " + path };
    }
    const json = parsed;
    if (json.hooks !== void 0 && (typeof json.hooks !== "object" || Array.isArray(json.hooks))) {
      return { ok: false, error: "hooks is not an object" };
    }
    if (!json.hooks) json.hooks = {};
    const guard = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then ';
    const authH = '-H "Authorization: Bearer $CODETERM_API_TOKEN"';
    const nonceH = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';
    const finish = " || true; fi";
    const notifyCmd = guard + `curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/permission" -H 'Content-Type: application/json' ` + authH + " -d '{}'" + finish;
    const stopCmd = guard + `curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/stop" -H 'Content-Type: application/json' ` + authH + " -d '{}'" + finish;
    const precompactCmd = guard + `curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/mem/flush" -H 'Content-Type: application/json' ` + authH + ` -d '{"tab":"'"$CODETERM_TAB_ID"'","title":"pre-compact"}'` + finish;
    const sessionStartCmd = guard + `curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" -H 'Content-Type: application/json' ` + authH + " " + nonceH + " --data-binary @-" + finish;
    function newEntry(matcher, cmd, timeout) {
      return {
        matcher,
        hooks: [{
          type: "command",
          command: cmd,
          timeout,
          _source: CODETERM_HOOK_MARKER
        }]
      };
    }
    const newHooks = {
      Notification: newEntry("permission_prompt", notifyCmd, 5e3),
      Stop: newEntry("", stopCmd, 5e3),
      SessionStart: newEntry("", sessionStartCmd, 5e3)
    };
    if (options.precompact_memory_export === true) {
      newHooks.PreCompact = newEntry("", precompactCmd, 1e4);
    }
    const keys = ["Notification", "Stop", "PreCompact", "SessionStart"];
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (json.hooks[k] !== void 0 && !Array.isArray(json.hooks[k])) {
        return { ok: false, error: "hooks." + k + " is not an array" };
      }
      const existing = Array.isArray(json.hooks[k]) ? json.hooks[k].filter((entry) => !isCodetermHook(entry)) : [];
      if (newHooks[k]) {
        existing.push(newHooks[k]);
        json.hooks[k] = existing;
      } else if (Array.isArray(json.hooks[k])) {
        json.hooks[k] = existing;
      }
    }
    if (!host.fs.writeFile(path + ".codeterm-backup", original) || !host.fs.writeFile(path + ".codeterm-backup-state", existed ? "present" : "absent")) {
      return { ok: false, error: "could not write recoverable backup for " + path };
    }
    const ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[claude-plugin] hooks installed at " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },
  uninstallIntegration() {
    const m = manifest();
    const path = host.expandHome(m && m.integration && m.integration.settingsPath || "~/.claude/settings.json");
    if (!path || !host.fileExists(path)) return { ok: true };
    const original = host.fs.readFile(path);
    if (typeof original !== "string") return { ok: false, error: "could not safely read settings JSON at " + path };
    let parsed;
    try {
      parsed = JSON.parse(original);
    } catch (_) {
      return { ok: false, error: "existing settings JSON is malformed at " + path };
    }
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return { ok: false, error: "existing settings JSON must contain an object at " + path };
    }
    const json = parsed;
    if (json.hooks === void 0) return { ok: true };
    if (!json.hooks || typeof json.hooks !== "object" || Array.isArray(json.hooks)) {
      return { ok: false, error: "hooks is not an object" };
    }
    const keys = ["Notification", "Stop", "PreCompact", "SessionStart"];
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (json.hooks[k] !== void 0 && !Array.isArray(json.hooks[k])) {
        return { ok: false, error: "hooks." + k + " is not an array" };
      }
    }
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      if (Array.isArray(json.hooks[k])) {
        json.hooks[k] = json.hooks[k].filter((entry) => !isCodetermHook(entry));
      }
    }
    const ok = host.fs.writeJson(path, json);
    if (ok) {
      host.log("info", "[claude-plugin] hooks removed from " + path);
      return { ok: true };
    }
    return { ok: false, error: "writeJson failed for " + path };
  },
  restoreIntegration() {
    const m = manifest();
    const path = host.expandHome(m && m.integration && m.integration.settingsPath || "~/.claude/settings.json");
    if (!path) return { ok: false, error: "could not expand settingsPath" };
    const backup = path + ".codeterm-backup";
    if (!host.fileExists(backup)) return { ok: false, error: "no recoverable backup at " + backup };
    const original = host.fs.readFile(backup);
    if (typeof original !== "string") return { ok: false, error: "restore failed for " + path };
    const state = host.fs.readFile(path + ".codeterm-backup-state");
    const wasAbsent = state === "absent" || typeof state !== "string" && original === "";
    if (wasAbsent) return !host.fileExists(path) || host.removeFile(path) ? { ok: true } : { ok: false, error: "restore failed for " + path };
    return host.fs.writeFile(path, original) ? { ok: true } : { ok: false, error: "restore failed for " + path };
  },
  // AS3/AS4 tab-state classification lives in ./classify; the facade delegates.
  // See that module for the auth/rate-limit/clarifying ensemble and its guarantees.
  classifyTabState(snapshot) {
    return classifyClaudeTabState(snapshot);
  }
};
function canonicalToolFields(name, input) {
  const tool = name.trim();
  const value = input && typeof input === "object" && !Array.isArray(input) ? input : {};
  const text = (key) => typeof value[key] === "string" ? value[key] : "";
  const command = text("command");
  const path = text("file_path") || text("filePath") || text("path");
  const pattern = text("pattern");
  const edits = extractCanonicalEdits(value);
  const lower = tool.toLowerCase();
  if (lower === "bash" || lower === "shell" || lower === "exec") {
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
  return {
    kind: "generic",
    summary: cleanToolText(tool, "Tool"),
    detail: tool,
    edits
  };
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
function writeTaskTempFile(task) {
  const suffix = ".ct-task-" + randomShortHex(8);
  const win = host.platform() === "windows";
  const body = win ? "\uFEFF" + task : task;
  return host.fs.writeTempFile(body, suffix);
}
function randomShortHex(len) {
  const hex = "0123456789abcdef";
  let out = "";
  for (let i = 0; i < len; i++) out += hex.charAt(Math.random() * 16 | 0);
  return out;
}
var CODETERM_HOOK_MARKER = "codeterm-hook-v3";
function isCodetermHook(entry) {
  const hooks = entry && entry.hooks;
  if (!Array.isArray(hooks)) return false;
  for (let i = 0; i < hooks.length; i++) {
    const h = hooks[i];
    if (!h) continue;
    if (typeof h._source === "string" && (h._source.indexOf("codeterm-hook") === 0 || h._source === "codeterm")) {
      return true;
    }
    const cmd = h.command;
    if (typeof cmd === "string") {
      const isUrl = cmd.indexOf("7685") !== -1 || cmd.indexOf("7686") !== -1;
      const isPath = cmd.indexOf("/api/notifications") !== -1 || cmd.indexOf("/api/tabs/") !== -1 || cmd.indexOf("/hook/permission") !== -1 || cmd.indexOf("/hook/stop") !== -1 || cmd.indexOf("/hook/session_start") !== -1 || cmd.indexOf("/api/mem/flush") !== -1;
      if (isUrl && isPath) return true;
    }
  }
  return false;
}
var plugin_default = plugin;
