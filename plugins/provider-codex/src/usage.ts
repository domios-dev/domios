interface CodexGauge {
  pct: number | null;
  resetsMs: number | null;
}

type CodexFamily = "general" | "legacy-session" | "weekly" | "model";

interface CodexLimitLine {
  family: CodexFamily;
  pct: number | null;
  resetsMs: number | null;
}

export function parseCodexUsage(rawText: string, nowMs: number): unknown {
  const raw = String(rawText || "");
  if (!raw.trim()) return null;
  const text = raw
    .split("\r")
    .join("")
    .split("\u2588")
    .join(" ")
    .split("\u2591")
    .join(" ");

  const lines = text.split("\n");
  const limits = parseCodexLimits(lines, nowMs);
  const session = limits.find((line) => line.family === "general") ?? limits.find((line) => line.family === "legacy-session");
  const weekly = limits.find((line) => line.family === "weekly");

  if (session === undefined && weekly === undefined) return null;

  return {
    provider: "codex",
    account_id: null,
    captured_at_ms: nowMs,
    session_pct: session ? session.pct : null,
    session_resets_at_ms: session ? session.resetsMs : null,
    weekly_pct: weekly ? weekly.pct : null,
    weekly_resets_at_ms: weekly ? weekly.resetsMs : null,
    raw_text: raw,
  };
}

function parseCodexLimits(lines: string[], nowMs: number): CodexLimitLine[] {
  const out: CodexLimitLine[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = String(lines[i] || "").trim();
    if (!line) continue;
    let context = line;
    for (let j = i - 1; j >= 0; j--) {
      const prev = String(lines[j] || "").trim();
      if (prev) {
        context = `${prev} ${line}`;
        break;
      }
    }
    const gauge = parseCodexGaugeLine(line, context, nowMs);
    if (gauge.pct === null && gauge.resetsMs === null) continue;

    const family = codexLimitFamily(context);
    if (family !== "model") {
      out.push({ family, pct: gauge.pct, resetsMs: gauge.resetsMs });
    }
  }

  return out;
}

function parseCodexGaugeLine(line: string, context: string, nowMs: number): CodexGauge {
  const re = /(\d+(?:\.\d+)?)\s*%\s*(left|remaining|used)\s*\(\s*resets?\s+(?:at\s+)?([^)]+?)\)/i;
  const caps = re.exec(line);
  if (!caps) {
    return { pct: null, resetsMs: null };
  }

  const value = parseFloat(caps[1]);
  const pct = codexPctToUsed(value, (caps[2] || "").toLowerCase());
  const resetsRaw = (caps[3] || "").trim();
  const isWeekly = /\bweekly\b/i.test(context);
  const resetsMs = isWeekly
    ? codexParseWeeklyResets(resetsRaw, nowMs)
    : codexParseSessionResets(resetsRaw, nowMs);
  return { pct, resetsMs };
}

function codexPctToUsed(value: number, mode: string): number | null {
  if (isNaN(value)) return null;
  const rawMode = mode === "used" ? "used" : "left";
  const pct = rawMode === "used" ? value : 100 - value;
  return Math.min(100, Math.max(0, pct));
}

function codexLimitFamily(text: string): CodexFamily {
  const lower = String(text || "").toLowerCase();
  const hasGeneral = /\bgeneral\b/.test(lower);
  const hasWeekly = /\bweekly\b/.test(lower);
  const hasSession = /\b5(?:\s*|-\s*)(?:h|hour|hours)\b/.test(lower);
  const hasModel = /\b(?:gpt|opus|haiku|sonnet|claude|codex|o\d|gemini)\b.*\d+\.\d+/.test(lower);

  if (hasModel) return "model";
  if (hasGeneral) return hasWeekly ? "weekly" : "general";
  if (hasWeekly) return "weekly";
  if (hasSession) return "legacy-session";
  return "general";
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
