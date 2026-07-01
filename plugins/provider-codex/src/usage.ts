interface CodexGauge {
  pct: number | null;
  resetsMs: number | null;
}

export function parseCodexUsage(rawText: string, nowMs: number): unknown {
  const raw = String(rawText || "");
  if (!raw.trim()) return null;
  const text = raw.split("\r").join("").split("█").join(" ").split("░").join(" ");
  const session = codexParseGauge(text, "5\\s*(?:h|hour|hours|-hour)\\s*limit", false, nowMs);
  const weekly = codexParseGauge(text, "weekly\\s*limit", true, nowMs);
  if (session.pct === null && weekly.pct === null) return null;
  return {
    provider: "codex",
    account_id: null,
    captured_at_ms: nowMs,
    session_pct: session.pct,
    session_resets_at_ms: session.resetsMs,
    weekly_pct: weekly.pct,
    weekly_resets_at_ms: weekly.resetsMs,
    raw_text: raw,
  };
}

function codexParseGauge(text: string, marker: string, isWeekly: boolean, nowMs: number): CodexGauge {
  const re = new RegExp(
    marker + "[\\s\\S]{0,240}?(\\d+(?:\\.\\d+)?)\\s*%\\s*(left|remaining|used)[\\s\\S]{0,180}?\\(\\s*resets?\\s+(?:at\\s+)?([^)]+?)\\)",
    "i",
  );
  const caps = re.exec(text);
  if (!caps) return { pct: null, resetsMs: null };
  const value = parseFloat(caps[1]);
  const mode = (caps[2] || "").toLowerCase();
  const pct = isNaN(value)
    ? null
    : Math.min(100, Math.max(0, mode === "used" ? value : 100 - value));
  const resetsRaw = (caps[3] || "").trim();
  const resetsMs = isWeekly
    ? codexParseWeeklyResets(resetsRaw, nowMs)
    : codexParseSessionResets(resetsRaw, nowMs);
  return { pct, resetsMs };
}

function codexParseSessionResets(raw: string, nowMs: number): number | null {
  const m = /(\d{1,2}):(\d{2})/.exec(String(raw || "").trim());
  if (!m) return null;
  const hh = parseInt(m[1], 10);
  const mm = parseInt(m[2], 10);
  const now = new Date(nowMs);
  let cand = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hh, mm, 0, 0);
  if (cand.getTime() <= nowMs) cand = new Date(cand.getTime() + 24 * 3600 * 1000);
  return cand.getTime();
}

const CODEX_MONTHS: Record<string, number> = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7,
  sep: 8, sept: 8, september: 8, oct: 9, october: 9, nov: 10, november: 10,
  dec: 11, december: 11,
};

function codexParseWeeklyResets(raw: string, nowMs: number): number | null {
  const s = String(raw || "").trim().replace(/^at\s+/i, "");
  let hh: number;
  let mm: number;
  let day: number;
  let month: number | undefined;
  let year: number | null = null;
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
  if (month === undefined) return null;
  const now = new Date(nowMs);
  let cand = new Date(year ?? now.getFullYear(), month, day, hh, mm, 0, 0);
  if (year === null && cand.getTime() <= nowMs) {
    cand = new Date(now.getFullYear() + 1, month, day, hh, mm, 0, 0);
  }
  return cand.getTime();
}
