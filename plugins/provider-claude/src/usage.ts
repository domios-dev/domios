// ── /usage screen parsing (port of the native usage_tui module) ─────────
// `usageNormalize` + `usageParseBlock` back the provider's parseUsage method;

export function usageNormalize(raw: string): string {
  let out = "";
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === "" || ch === "\r") continue;
    if ("█▌▍▎▏▊▋".indexOf(ch) !== -1) { out += " "; continue; }
    out += ch;
  }
  return out;
}

interface UsageBlock { pct: number | null; resetsMs: number | null }

interface UsageHost {
  expandHome(path: string): string | null;
  envGet(name: string): string | null;
  platform(): string;
  exec(opts: string): string;
  fetch(opts: string): string;
  fs: { readFile(path: string): string | null };
}

const CLAUDE_USAGE_KIND = "codeterm_usage_v1";

export function fetchClaudeUsage(host: UsageHost): string {
  const credentialsPath = host.expandHome("~/.claude/.credentials.json");
  const fileCredentials = credentialsPath ? safeJson(host.fs.readFile(credentialsPath)) : null;
  const credentials = readMacOsCredentials(host) ?? fileCredentials;
  const oauth = objectAt(credentials, "claudeAiOauth");
  const accessToken = stringAt(oauth, "accessToken");
  if (!accessToken) throw new Error("usage credentials unavailable");
  const response = safeJson(host.fetch(JSON.stringify({
    url: "https://api.anthropic.com/api/oauth/usage",
    method: "GET",
    timeoutMs: 6000,
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "anthropic-beta": "oauth-2025-04-20",
      "User-Agent": "codeterm-usage/1.7.8",
    },
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
    weekly_resets_at_ms: isoToMs(stringAt(weekly, "resets_at")),
  });
}

function readMacOsCredentials(host: UsageHost): Record<string, unknown> | null {
  if (host.platform() !== "macos") return null;
  const user = host.envGet("USER");
  const args = ["find-generic-password"];
  if (user) args.push("-a", user);
  args.push("-w", "-s", "Claude Code-credentials");
  const result = safeJson(host.exec(JSON.stringify({ bin: "security", args, timeoutMs: 5000 })));
  if (numberAt(result, "code") !== 0) return null;
  return safeJson(stringAt(result, "stdout"));
}

export function parseFetchedClaudeUsage(raw: string, nowMs: number): unknown | undefined {
  const fetched = safeJson(raw);
  if (stringAt(fetched, "kind") !== CLAUDE_USAGE_KIND) return undefined;
  return {
    provider: "claude",
    account_id: null,
    captured_at_ms: nowMs,
    session_pct: numberAt(fetched, "session_pct"),
    session_resets_at_ms: numberAt(fetched, "session_resets_at_ms"),
    weekly_pct: numberAt(fetched, "weekly_pct"),
    weekly_resets_at_ms: numberAt(fetched, "weekly_resets_at_ms"),
    raw_text: raw,
  };
}

function safeJson(raw: unknown): Record<string, unknown> | null {
  if (typeof raw !== "string" || !raw.trim()) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

function objectAt(value: unknown, key: string): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  const nested = (value as Record<string, unknown>)[key];
  return nested && typeof nested === "object" ? nested as Record<string, unknown> : null;
}

function stringAt(value: unknown, key: string): string | null {
  if (!value || typeof value !== "object") return null;
  const nested = (value as Record<string, unknown>)[key];
  return typeof nested === "string" && nested ? nested : null;
}

function numberAt(value: unknown, key: string): number | null {
  if (!value || typeof value !== "object") return null;
  const nested = (value as Record<string, unknown>)[key];
  return typeof nested === "number" && Number.isFinite(nested) ? nested : null;
}

function isoToMs(value: string | null): number | null {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function usageBlockIsComplete(block: UsageBlock): boolean {
  return block.pct === null || block.resetsMs !== null;
}

export function usageParseBlock(text: string, marker: string, isWeekly: boolean, nowMs: number): UsageBlock {
  const markerMatch = new RegExp(marker).exec(text);
  if (!markerMatch) return { pct: null, resetsMs: null };
  const tail = text.slice(markerMatch.index + markerMatch[0].length, markerMatch.index + markerMatch[0].length + 800);
  const nextBlock = /\n\s*Current\s+/i.exec(tail);
  const block = nextBlock ? tail.slice(0, nextBlock.index) : tail;
  const pctCaps = /(\d+(?:\.\d+)?)\s*%\s*used/.exec(block);
  if (!pctCaps) return { pct: null, resetsMs: null };
  let pct: number | null = parseFloat(pctCaps[1]);
  if (isNaN(pct)) pct = null;
  const resetCaps = /Resets?\s*([^()\n]+?)\s*\(/.exec(block);
  const timeRaw = (resetCaps?.[1] || "").trim();
  const resetsMs = isWeekly
    ? usageParseWeeklyReset(timeRaw, nowMs)
    : usageParseTodayClock(timeRaw, nowMs);
  return { pct: pct, resetsMs: resetsMs };
}

interface HMmAmPm { hh: number; mm: number; pm: boolean }

function usageParseHMmAmPm(s: string): HMmAmPm | null {
  s = String(s || "").trim().toLowerCase();
  let pm: boolean;
  if (s.slice(-2) === "pm") pm = true;
  else if (s.slice(-2) === "am") pm = false;
  else return null;
  const stripped = s.slice(0, -2).trim();
  let hh: number, mm: number;
  const idx = stripped.indexOf(":");
  if (idx !== -1) {
    hh = parseInt(stripped.slice(0, idx).trim(), 10);
    mm = parseInt(stripped.slice(idx + 1).trim(), 10);
  } else {
    hh = parseInt(stripped, 10);
    mm = 0;
  }
  if (isNaN(hh) || isNaN(mm) || hh === 0 || hh > 12 || mm > 59) return null;
  return { hh: hh, mm: mm, pm: pm };
}

export function usageParseTodayClock(s: string, nowMs: number): number | null {
  const t = usageParseHMmAmPm(s);
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour24, t.mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1000);
  return cand.getTime();
}

function usageParseWeeklyReset(s: string, nowMs: number): number | null {
  return usageParseMonthDayClock(s, nowMs)
    ?? usageParseRelativeDayClock(s, nowMs)
    ?? usageParseTodayClock(s, nowMs);
}

const USAGE_MONTHS: Record<string, number> = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7,
  sep: 8, sept: 8, september: 8, oct: 9, october: 9, nov: 10, november: 10,
  dec: 11, december: 11,
};

function usageParseMonthDayClock(s: string, nowMs: number): number | null {
  const parts = usageCleanParts(s);
  while (parts.length > 0 && usageIsWeekday(parts[0])) parts.shift();
  if (parts.length < 3) return null;
  const month = USAGE_MONTHS[parts[0]];
  if (month === undefined) return null;
  const day = usageParseDayToken(parts[1]);
  if (day === null) return null;
  let idx = 2;
  let explicitYear: number | null = null;
  if (/^\d{4}$/.test(parts[idx] || "")) {
    explicitYear = parseInt(parts[idx], 10);
    idx++;
  }
  if (parts[idx] === "at") idx++;
  const t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  const year = explicitYear ?? now.getFullYear();
  let cand = new Date(year, month, day, hour24, t.mm, 0, 0);
  if (explicitYear === null && cand.getTime() < nowMs) {
    cand = new Date(year + 1, month, day, hour24, t.mm, 0, 0);
  }
  return cand.getTime();
}

function usageParseRelativeDayClock(s: string, nowMs: number): number | null {
  const parts = usageCleanParts(s);
  const first = parts[0];
  let offsetDays: number;
  if (first === "today" || first === "tonight") offsetDays = 0;
  else if (first === "tomorrow") offsetDays = 1;
  else return null;
  let idx = 1;
  if (parts[idx] === "at") idx++;
  const t = usageParseHMmAmPm(parts.slice(idx).join(" "));
  if (!t) return null;
  const hour24 = (t.hh % 12) + (t.pm ? 12 : 0);
  const now = new Date(nowMs);
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + offsetDays,
    hour24,
    t.mm,
    0,
    0,
  ).getTime();
}

function usageCleanParts(s: string): string[] {
  return String(s || "")
    .trim()
    .toLowerCase()
    .replace(/,/g, " ")
    .split(/\s+/)
    .map((p) => p.replace(/[.;]+$/g, ""))
    .filter(Boolean);
}

function usageParseDayToken(s: string): number | null {
  const n = parseInt(String(s || "").replace(/(?:st|nd|rd|th)$/i, ""), 10);
  return Number.isFinite(n) && n >= 1 && n <= 31 ? n : null;
}

function usageIsWeekday(s: string): boolean {
  return /^(mon|monday|tue|tues|tuesday|wed|wednesday|thu|thur|thurs|thursday|fri|friday|sat|saturday|sun|sunday)$/.test(s);
}
