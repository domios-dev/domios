import { blockSource, featureInUse, type HelperBlock, type HelperFacts } from "./helperBlocks";
import type { MomentId } from "./momentRegistry";

export interface MomentRecord {
  shown: number;
  accepted: number;
  dismissed: number;
  muted: boolean;
  seen: readonly string[];
}

export interface HelperMemory {
  moments: Partial<Record<MomentId, MomentRecord>>;
}

export interface HelperSuggestion {
  momentId: MomentId;
  block: HelperBlock;
  benefit: string;
}

export type HelperOutcome = "accepted" | "dismissed" | "muted";

export type AdmitVerdict =
  | { admit: true }
  | { admit: false; reason: "busy" | "muted" | "retired" | "seen" | "in-use" | "unknown-use" };

export const EMPTY_MEMORY: HelperMemory = { moments: {} };

export const RETIREMENT = { minShown: 3, dismissRate: 2 / 3 };

const EMPTY_RECORD: MomentRecord = { shown: 0, accepted: 0, dismissed: 0, muted: false, seen: [] };

export function momentRecord(memory: HelperMemory, momentId: MomentId): MomentRecord {
  return memory.moments[momentId] ?? EMPTY_RECORD;
}

/** Each suggestion is identified by its moment and block, so it shows once. */
export function suggestionKey(suggestion: Pick<HelperSuggestion, "momentId" | "block">): string {
  return `${suggestion.momentId}|${blockSource(suggestion.block)}`;
}

/** A moment retires itself once people mostly dismiss it. */
export function isRetired(record: MomentRecord): boolean {
  return record.shown >= RETIREMENT.minShown && record.dismissed / record.shown >= RETIREMENT.dismissRate;
}

/** The helper rules: one suggestion at a time, never muted, retired, repeated, or for a feature already in use. */
export function admitSuggestion(memory: HelperMemory, active: HelperSuggestion | null, candidate: HelperSuggestion, facts: HelperFacts): AdmitVerdict {
  if (active) return { admit: false, reason: "busy" };
  const record = momentRecord(memory, candidate.momentId);
  if (record.muted) return { admit: false, reason: "muted" };
  if (isRetired(record)) return { admit: false, reason: "retired" };
  if (record.seen.includes(suggestionKey(candidate))) return { admit: false, reason: "seen" };
  const inUse = featureInUse(candidate.block, facts);
  if (inUse === null) return { admit: false, reason: "unknown-use" };
  return inUse ? { admit: false, reason: "in-use" } : { admit: true };
}

function withRecord(memory: HelperMemory, momentId: MomentId, update: (record: MomentRecord) => MomentRecord): HelperMemory {
  return { moments: { ...memory.moments, [momentId]: update(momentRecord(memory, momentId)) } };
}

export function recordShown(memory: HelperMemory, suggestion: HelperSuggestion): HelperMemory {
  const key = suggestionKey(suggestion);
  if (momentRecord(memory, suggestion.momentId).seen.includes(key)) return memory;
  return withRecord(memory, suggestion.momentId, (record) => ({ ...record, shown: record.shown + 1, seen: [...record.seen, key] }));
}

/** "Don't show again" counts as a dismissal and silences the moment for good. */
export function recordOutcome(memory: HelperMemory, suggestion: HelperSuggestion, outcome: HelperOutcome): HelperMemory {
  return withRecord(memory, suggestion.momentId, (record) => outcome === "accepted"
    ? { ...record, accepted: record.accepted + 1 }
    : { ...record, dismissed: record.dismissed + 1, muted: record.muted || outcome === "muted" });
}

const MOMENT_IDS: readonly MomentId[] = [
  "waiting_on_user_while_away",
  "service_without_plugin",
  "long_task_finished_unattended",
  "repeated_manual_action",
  "adjacent_capability",
];

function count(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? Math.floor(value) : 0;
}

/** Restores persisted memory, dropping anything malformed instead of trusting it. */
export function parseHelperMemory(raw: unknown): HelperMemory {
  const moments = (raw as { moments?: Record<string, unknown> } | null)?.moments;
  if (!moments || typeof moments !== "object") return EMPTY_MEMORY;
  const restored: HelperMemory["moments"] = {};
  for (const id of MOMENT_IDS) {
    const entry = moments[id] as Partial<MomentRecord> | undefined;
    if (!entry || typeof entry !== "object") continue;
    restored[id] = {
      shown: count(entry.shown),
      accepted: count(entry.accepted),
      dismissed: count(entry.dismissed),
      muted: entry.muted === true,
      seen: Array.isArray(entry.seen) ? entry.seen.filter((key): key is string => typeof key === "string") : [],
    };
  }
  return { moments: restored };
}
