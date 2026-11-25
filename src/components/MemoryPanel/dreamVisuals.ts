export interface ParsedDream {
  insight: string;
  log: string | null;
  icon: string | null;
  mood: string | null;
}

const INSIGHT_RE = /<insight>([\s\S]*?)<\/insight>/i;
const LOG_RE = /<log>([\s\S]*?)<\/log>/i;
const ICON_RE = /<icon>\s*([a-z][a-z0-9-]*)\s*<\/icon>/i;
const MOOD_RE = /<mood>\s*([^<\n]+?)\s*<\/mood>/i;

// Parse dream body blocks (`<insight>`, `<log>`, `<icon>`, `<mood>`).
// If none are present, treat full body as insight.
export function parseDream(body: string): ParsedDream {
  if (!body) return { insight: "", log: null, icon: null, mood: null };
  const insightMatch = body.match(INSIGHT_RE);
  const logMatch = body.match(LOG_RE);
  const iconMatch = body.match(ICON_RE);
  const moodMatch = body.match(MOOD_RE);
  if (insightMatch || logMatch || iconMatch || moodMatch) {
    return {
      insight: insightMatch ? insightMatch[1].trim() : "",
      log: logMatch ? logMatch[1].trim() : null,
      icon: iconMatch ? iconMatch[1].trim().toLowerCase() : null,
      mood: moodMatch ? moodMatch[1].trim() : null,
    };
  }
  return { insight: body.trim(), log: null, icon: null, mood: null };
}

export function hasRenderableDreamInsight(body: string | null | undefined): boolean {
  if (!body) return false;
  return parseDream(body).insight.trim().length > 0;
}

export interface DreamSummary {
  text: string;
  source: "report" | "log";
}

interface CounterValue {
  lead: number | null;
  keyed: Record<string, number>;
}

interface CounterPhrase {
  label: string;
  // null reads the line's leading number, or its only key=N pair.
  key: string | null;
  verb: string;
  one: string;
  many: string;
}

// Diary <log> counters as the REM prompt writes them; zero or unfilled values drop out.
const COUNTER_PHRASES: CounterPhrase[] = [
  { label: "DREAM_BACKLINKS", key: null, verb: "pruned", one: "dream backlink", many: "dream backlinks" },
  { label: "PARALLEL_EDGES", key: null, verb: "collapsed", one: "parallel edge", many: "parallel edges" },
  { label: "ISLANDS", key: "bridged", verb: "bridged", one: "island", many: "islands" },
  { label: "ISLANDS", key: "promoted", verb: "promoted", one: "island", many: "islands" },
  { label: "ISLANDS", key: "deleted", verb: "deleted", one: "island", many: "islands" },
  { label: "ORPHANS", key: "bridged", verb: "linked", one: "orphan", many: "orphans" },
  { label: "ORPHANS", key: "deleted", verb: "deleted", one: "orphan", many: "orphans" },
  { label: "DEDUP", key: "aliases", verb: "merged", one: "alias", many: "aliases" },
  { label: "DEDUP", key: "duplicates", verb: "removed", one: "duplicate", many: "duplicates" },
  { label: "VECTOR_HYGIENE", key: null, verb: "pruned", one: "thin vector", many: "thin vectors" },
];

function logCounters(log: string): Map<string, CounterValue> {
  const counters = new Map<string, CounterValue>();
  for (const line of log.split("\n")) {
    const colon = line.indexOf(":");
    if (colon < 0) continue;
    const label = line.slice(0, colon).trim();
    if (!/^[A-Z_]+$/.test(label) || counters.has(label)) continue;
    const value = line.slice(colon + 1).trim();
    const lead = value.match(/^(\d+)\b/);
    const keyed: Record<string, number> = {};
    for (const m of value.matchAll(/([a-z_]+)=(\d+)/g)) keyed[m[1]] = Number(m[2]);
    counters.set(label, { lead: lead ? Number(lead[1]) : null, keyed });
  }
  return counters;
}

function counterAmount(value: CounterValue, key: string | null): number | null {
  if (key) return value.keyed[key] ?? null;
  if (value.lead !== null) return value.lead;
  const pairs = Object.values(value.keyed);
  return pairs.length === 1 ? pairs[0] : null;
}

function hubsPhrase(counters: Map<string, CounterValue>): string | null {
  const before = counters.get("HUBS_BEFORE")?.lead ?? null;
  const after = counters.get("HUBS_AFTER")?.lead ?? null;
  if (before === null || after === null || before === after) return null;
  return `hubs ${before} → ${after}`;
}

export function summarizeDreamLog(log: string | null | undefined): string | null {
  if (!log) return null;
  const counters = logCounters(log);
  const phrases: string[] = [];
  const hubs = hubsPhrase(counters);
  if (hubs) phrases.push(hubs);
  for (const spec of COUNTER_PHRASES) {
    const value = counters.get(spec.label);
    const n = value ? counterAmount(value, spec.key) : null;
    if (!n) continue;
    phrases.push(`${spec.verb} ${n} ${n === 1 ? spec.one : spec.many}`);
  }
  if (phrases.length === 0) return null;
  const text = phrases.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

// The worker's own report wins; the diary counters are the fallback; otherwise nothing.
export function dreamSummary(input: {
  report?: string | null;
  log?: string | null;
}): DreamSummary | null {
  const reportLine = (input.report ?? "")
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);
  if (reportLine) return { text: reportLine, source: "report" };
  const derived = summarizeDreamLog(input.log);
  return derived ? { text: derived, source: "log" } : null;
}
