// ── /usage screen parsing (port of the native usage_tui module) ─────────
//
// `usageNormalize` + `usageParseBlock` back the provider's parseUsage method;
// `usageParseTodayClock` is reused by the AS3 rate-limit reset-clock parser.

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

export function usageParseBlock(text: string, marker: string, isWeekly: boolean, nowMs: number): UsageBlock {
  const re = new RegExp(
    marker + "[\\s\\S]{0,800}?(\\d+(?:\\.\\d+)?)\\s*%\\s*used[\\s\\S]*?Resets?\\s*([^()\\n]+?)\\s*\\(",
  );
  const caps = re.exec(text);
  if (!caps) return { pct: null, resetsMs: null };
  let pct: number | null = parseFloat(caps[1]);
  if (isNaN(pct)) pct = null;
  const timeRaw = (caps[2] || "").trim();
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
