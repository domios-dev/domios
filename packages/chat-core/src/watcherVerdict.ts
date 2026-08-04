/** TS mirror of `src-tauri/src/service/watcher/verdict.rs::parse_verdict`. */

export type WatcherVerdictStatus = "ok" | "attention" | "stalled";

export interface WatcherVerdictAction {
  kind: string;
  tab?: string;
  message: string;
}

export interface ParsedWatcherVerdict {
  status: WatcherVerdictStatus;
  summary: string;
  actions: WatcherVerdictAction[];
}

const MALFORMED_SUMMARY_MAX = 500;
const FENCE_JSON_RE = /```(?:json)?\s*\r?\n?([\s\S]*?)```/gi;

function rawPrefix(raw: string, max: number): string {
  return [...raw].slice(0, max).join("");
}

function parseStatus(s: string): WatcherVerdictStatus | null {
  switch (s.toLowerCase()) {
    case "ok":
      return "ok";
    case "attention":
      return "attention";
    case "stalled":
      return "stalled";
    default:
      return null;
  }
}

function parseActions(v: unknown): WatcherVerdictAction[] {
  if (!Array.isArray(v)) return [];
  const actions: WatcherVerdictAction[] = [];
  for (const item of v) {
    if (!item || typeof item !== "object") continue;
    const row = item as Record<string, unknown>;
    const kind = typeof row.kind === "string" ? row.kind : "";
    const message = typeof row.message === "string" ? row.message : "";
    if (!kind || !message) continue;
    const tab = typeof row.tab === "string" ? row.tab : undefined;
    actions.push({ kind, tab, message });
  }
  return actions;
}

function parseObjectAt(text: string, start: number): { value: Record<string, unknown>; end: number } | null {
  if (!text.startsWith("{", start)) return null;
  let depth = 0;
  let inString = false;
  let escape = false;
  for (let idx = start; idx < text.length; idx += 1) {
    const ch = text[idx];
    if (inString) {
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "{") depth += 1;
    else if (ch === "}") {
      depth -= 1;
      if (depth === 0) {
        const slice = text.slice(start, idx + 1);
        try {
          const parsed = JSON.parse(slice) as unknown;
          if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
            return { value: parsed as Record<string, unknown>, end: idx + 1 };
          }
        } catch {
          return null;
        }
        return null;
      }
    }
  }
  return null;
}

function extractJsonObject(raw: string): Record<string, unknown> | null {
  const trimmed = raw.trim();
  if (trimmed) {
    try {
      const parsed = JSON.parse(trimmed) as unknown;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch {
      // fall through
    }
  }
  for (const match of raw.matchAll(FENCE_JSON_RE)) {
    const body = match[1]?.trim();
    if (!body) continue;
    try {
      const parsed = JSON.parse(body) as unknown;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return parsed as Record<string, unknown>;
      }
    } catch {
      // try next fence
    }
  }
  let best: { value: Record<string, unknown>; len: number } | null = null;
  for (let idx = 0; idx < raw.length; idx += 1) {
    if (raw[idx] !== "{") continue;
    const parsed = parseObjectAt(raw, idx);
    if (!parsed) continue;
    const len = parsed.end - idx;
    if (!best || len > best.len) best = { value: parsed.value, len };
  }
  return best?.value ?? null;
}

function parseVerdictValue(value: Record<string, unknown>, rawFallback: string): ParsedWatcherVerdict {
  const status = typeof value.status === "string" ? parseStatus(value.status) : null;
  if (!status) {
    const summary =
      typeof value.summary === "string"
        ? value.summary
        : rawPrefix(rawFallback, MALFORMED_SUMMARY_MAX);
    return {
      status: "attention",
      summary,
      actions: parseActions(value.actions),
    };
  }

  const summaryField = typeof value.summary === "string" ? value.summary.trim() : "";
  const summary =
    summaryField.length > 0 ? summaryField : rawPrefix(rawFallback, MALFORMED_SUMMARY_MAX);

  return {
    status,
    summary,
    actions: parseActions(value.actions),
  };
}

export function parseWatcherVerdictContent(raw: string): ParsedWatcherVerdict {
  const value = extractJsonObject(raw);
  if (!value) {
    return {
      status: "attention",
      summary: rawPrefix(raw, MALFORMED_SUMMARY_MAX),
      actions: [],
    };
  }
  return parseVerdictValue(value, raw);
}
