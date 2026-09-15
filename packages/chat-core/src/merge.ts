import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

const MIN_SEMANTIC_BATCH_SIZE = 2;
const semanticFingerprintCache = new WeakMap<object, string>();

function stableValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(stableValue);
  if (value == null || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter(([, entry]) => entry !== undefined)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, entry]) => [key, stableValue(entry)]),
  );
}

function semanticMessageFingerprint(message: NormalizedChatMessage): string {
  const cached = semanticFingerprintCache.get(message);
  if (cached) return cached;
  const {
    id: _id,
    seq: _seq,
    toolId: _toolId,
    ...semantic
  } = message;
  const fingerprint = JSON.stringify(stableValue({
    ...semantic,
    content: semantic.content.replace(/\r\n/g, "\n"),
    attachments: semantic.attachments?.map(({ id: _attachmentId, ...attachment }) => attachment),
    interactive: semantic.interactive?.map(({ id: _elementId, toolUseId: _toolUseId, ...element }) => element),
  }));
  semanticFingerprintCache.set(message, fingerprint);
  return fingerprint;
}

function longestBoundaryOverlap(previous: readonly string[], incoming: readonly string[]): number {
  const prefix = new Array<number>(incoming.length).fill(0);
  for (let index = 1; index < incoming.length; index += 1) {
    let matched = prefix[index - 1] ?? 0;
    while (matched > 0 && incoming[index] !== incoming[matched]) matched = prefix[matched - 1] ?? 0;
    if (incoming[index] === incoming[matched]) matched += 1;
    prefix[index] = matched;
  }
  let matched = 0;
  for (const key of previous.slice(-incoming.length)) {
    while (matched > 0 && key !== incoming[matched]) matched = prefix[matched - 1] ?? 0;
    if (key === incoming[matched]) matched += 1;
  }
  return matched;
}

/** Duplicate transport projections arrive as a batch, not as isolated content
 * repeats. Drop only an identical multi-row batch or boundary overlap. */
function duplicateIncomingIndexes<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): Set<number> {
  const duplicates = new Set<number>();
  if (incoming.length < MIN_SEMANTIC_BATCH_SIZE) return duplicates;
  const incomingKeys = incoming.map(semanticMessageFingerprint);
  const previousTailKeys = prev.slice(-incoming.length).map(semanticMessageFingerprint);
  const overlap = longestBoundaryOverlap(previousTailKeys, incomingKeys);
  if (overlap >= MIN_SEMANTIC_BATCH_SIZE) {
    for (let index = 0; index < overlap; index += 1) duplicates.add(index);
  }

  if (incoming.length % 2 === 0) {
    const half = incoming.length / 2;
    let halvesMatch = half >= MIN_SEMANTIC_BATCH_SIZE;
    for (let index = 0; halvesMatch && index < half; index += 1) {
      halvesMatch = incomingKeys[index] === incomingKeys[half + index];
    }
    if (halvesMatch) {
      for (let index = half; index < incoming.length; index += 1) duplicates.add(index);
    }
  }
  return duplicates;
}

/** `seq` is a per-batch ordinal that other batches reuse for unrelated rows,
 *  so only the provider id identifies a row here. */
function mergeIdentities(message: NormalizedChatMessage): string[] {
  return [`id:${message.id}`];
}

/** Merge a freshly-arrived batch into the existing transcript: dedupe by stable
 *  `id` (incoming wins, so a re-emitted message updates in place) then order by
 *  `(timestamp, seq, arrival)`.
 *
 *  Fallback for sources with no byte model; ranged sources reconcile in
 *  `reduceTranscript`.
 *
 *  The ordering contract is provider-agnostic. Two signals can be missing or
 *  coarse, so neither alone is trusted:
 *    - `timestamp` — codex floors to whole seconds and emits ISO strings; cursor
 *      emits none at all. So we only let time decide when BOTH sides parse to a
 *      finite epoch-ms AND differ (a raw `Number("2026-..ISO..")` is NaN — the
 *      old comparator's silent no-op that let same-second turns shuffle).
 *    - `seq` — the server's monotonic source ordinal (JSONL rich-message index /
 *      structured row position). It is the stable tiebreaker for equal/missing
 *      timestamps, falling back to arrival order when a provider supplies none. */
export function mergeById<T extends NormalizedChatMessage>(prev: T[], incoming: T[]): T[] {
  const semanticDuplicates = duplicateIncomingIndexes(prev, incoming);
  const slots = new Map<string, number>();
  const entries: Array<{ message: T; arrival: number }> = [];
  const projectedIncoming = incoming.filter((_, index) => !semanticDuplicates.has(index));
  for (const message of [...prev, ...projectedIncoming]) {
    const [identity] = mergeIdentities(message);
    const slot = slots.get(identity);
    if (slot != null) {
      entries[slot] = { message, arrival: entries[slot].arrival };
      continue;
    }
    slots.set(identity, entries.length);
    entries.push({ message, arrival: entries.length });
  }
  return entries
    .sort((a, b) => {
      const at = timestampMs(a.message.timestamp);
      const bt = timestampMs(b.message.timestamp);
      if (at != null && bt != null && at !== bt) return at - bt;
      const as = a.message.seq;
      const bs = b.message.seq;
      if (as != null && bs != null && as !== bs) return as - bs;
      return a.arrival - b.arrival;
    })
    .map((entry) => entry.message);
}
