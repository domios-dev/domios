import { matchedOptimisticEchoes, type OptimisticEcho } from "./dedupe";
import { mergeById } from "./merge";
import type { NormalizedChatMessage } from "./types";

/** Half-open byte range `[start, end)` of a session source, both ends on a
 *  complete-record boundary. This is the only cross-batch identity signal:
 *  row ids and `seq` are ordinals inside one parsed batch and mean nothing
 *  outside it. */
export interface TranscriptRange {
  start: number;
  end: number;
}

export interface TranscriptAuthority {
  sourceSessionId: string | null;
  sessionBindingNonce: string | null;
}

export type TranscriptBatchKind = "snapshot" | "delta" | "backfill";

export interface TranscriptSegment {
  kind: TranscriptBatchKind;
  /** `null` for sources with no byte model (structured/store-backed providers). */
  range: TranscriptRange | null;
  rows: NormalizedChatMessage[];
}

export interface TranscriptState {
  authority: TranscriptAuthority | null;
  segments: TranscriptSegment[];
  /** Rows dropped off the top by the retention bound. */
  evictedOlder: number;
}

export interface TranscriptBatch {
  kind: TranscriptBatchKind;
  authority: TranscriptAuthority;
  range: TranscriptRange | null;
  rows: NormalizedChatMessage[];
}

export type TranscriptOutcome =
  /** Committed into the transcript. */
  | "adopted"
  /** Session changed: the whole prior transcript was replaced. */
  | "rotated"
  /** Byte range proves the rows are already committed. */
  | "ignored_stale"
  | "ignored_empty"
  /** Malformed or regressing range — state kept, nothing adopted. */
  | "rejected_range"
  /** Adopted, but a byte gap below it needs an authoritative snapshot. */
  | "recover_gap"
  /** Overlap that no byte range proves — dropped, needs a snapshot. */
  | "recover_overlap";

export interface TranscriptResult {
  state: TranscriptState;
  outcome: TranscriptOutcome;
}

/** Hard bound on retained source rows; the oldest are evicted first. */
export const TRANSCRIPT_RETENTION_LIMIT = 440;

export function createTranscriptState(): TranscriptState {
  return { authority: null, segments: [], evictedOlder: 0 };
}

function isRange(range: TranscriptRange | null | undefined): range is TranscriptRange {
  if (range == null) return false;
  const { start, end } = range;
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end)) return false;
  return start >= 0 && end >= start;
}

function sameAuthority(left: TranscriptAuthority, right: TranscriptAuthority): boolean {
  return left.sourceSessionId === right.sourceSessionId
    && left.sessionBindingNonce === right.sessionBindingNonce;
}

function rangedSegments(state: TranscriptState): TranscriptSegment[] {
  return state.segments.filter((segment) => segment.range != null);
}

/** End of the committed byte span — the live cursor. `null` when nothing
 *  ranged is committed yet. */
export function transcriptCommittedEnd(state: TranscriptState): number | null {
  let end: number | null = null;
  for (const segment of rangedSegments(state)) {
    const candidate = segment.range as TranscriptRange;
    if (end == null || candidate.end > end) end = candidate.end;
  }
  return end;
}

/** Start of the committed byte span — the `chat_backfill` paging cursor. */
export function transcriptBackfillCursor(state: TranscriptState): number | null {
  let start: number | null = null;
  for (const segment of rangedSegments(state)) {
    const candidate = segment.range as TranscriptRange;
    if (start == null || candidate.start < start) start = candidate.start;
  }
  return start;
}

function orderedSegments(segments: TranscriptSegment[]): TranscriptSegment[] {
  return [...segments].sort((left, right) => {
    const leftStart = left.range?.start ?? Number.MAX_SAFE_INTEGER;
    const rightStart = right.range?.start ?? Number.MAX_SAFE_INTEGER;
    return leftStart - rightStart;
  });
}

/** Committed rows in source order. Segments never overlap by construction, so
 *  this is a concatenation; a repeated id can only come from an unranged
 *  source, where the newest occurrence wins. */
export function transcriptRows(state: TranscriptState): NormalizedChatMessage[] {
  const rows: NormalizedChatMessage[] = [];
  for (const segment of orderedSegments(state.segments)) rows.push(...segment.rows);
  const seen = new Set<string>();
  const out: NormalizedChatMessage[] = [];
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const row = rows[index];
    if (seen.has(row.id)) continue;
    seen.add(row.id);
    out.push(row);
  }
  return out.reverse();
}

export function transcriptRowCount(state: TranscriptState): number {
  return state.segments.reduce((total, segment) => total + segment.rows.length, 0);
}

/** Evict whole oldest segments only, and only while the remaining ones still
 *  satisfy the bound. Slicing rows out of a segment would leave its byte range
 *  describing rows it no longer holds — the paging cursor would then point
 *  below the evicted rows and they could never be recovered. Whole-segment
 *  eviction keeps every retained range exact, so one page ending at the new
 *  cursor restores exactly what was dropped. */
function withRetentionBound(state: TranscriptState, limit: number): TranscriptState {
  let kept = orderedSegments(state.segments);
  let total = transcriptRowCount(state);
  let evicted = 0;
  while (kept.length > 1 && total > limit) {
    const oldest = kept[0];
    if (total - oldest.rows.length < limit) break;
    total -= oldest.rows.length;
    evicted += oldest.rows.length;
    kept = kept.slice(1);
  }
  if (evicted === 0) return state;
  return { ...state, segments: kept, evictedOlder: state.evictedOlder + evicted };
}

function committed(
  state: TranscriptState,
  segments: TranscriptSegment[],
  outcome: TranscriptOutcome,
  limit: number,
): TranscriptResult {
  return {
    state: withRetentionBound({ ...state, segments: orderedSegments(segments) }, limit),
    outcome,
  };
}

function unrangedRows(state: TranscriptState): NormalizedChatMessage[] {
  const rows: NormalizedChatMessage[] = [];
  for (const segment of state.segments) {
    if (segment.range == null) rows.push(...segment.rows);
  }
  return rows;
}

function withUnrangedRows(
  state: TranscriptState,
  kind: TranscriptBatchKind,
  rows: NormalizedChatMessage[],
): TranscriptSegment[] {
  return [
    ...state.segments.filter((segment) => segment.range != null),
    { kind, range: null, rows },
  ];
}

function reduceSnapshot(
  state: TranscriptState,
  batch: TranscriptBatch,
  limit: number,
): TranscriptResult {
  if (!isRange(batch.range)) {
    const rows = mergeById(unrangedRows(state), batch.rows);
    return committed(state, withUnrangedRows(state, "snapshot", rows), "adopted", limit);
  }
  const range = batch.range;
  // A committed segment straddling the window end would be half re-parsed and
  // half not: nothing proves which of its rows the snapshot re-delivers, so a
  // window that lands there (a stale parse arriving late) is refused.
  const straddlesEnd = state.segments.some(
    (segment) => segment.range != null
      && segment.range.start < range.end
      && range.end < segment.range.end,
  );
  if (straddlesEnd) return { state, outcome: "rejected_range" };
  // The snapshot is authoritative for its own window only: strictly-older
  // segments and a strictly-newer live tail both survive it. Rows with no byte
  // range are never dropped either — nothing proves this window covers them.
  const preserved = state.segments.filter(
    (segment) => segment.range == null
      || segment.range.end <= range.start
      || segment.range.start >= range.end,
  );
  return committed(
    state,
    [...preserved, { kind: "snapshot", range, rows: batch.rows }],
    "adopted",
    limit,
  );
}

function reduceDelta(
  state: TranscriptState,
  batch: TranscriptBatch,
  limit: number,
): TranscriptResult {
  if (!isRange(batch.range)) {
    const rows = mergeById(unrangedRows(state), batch.rows);
    return committed(state, withUnrangedRows(state, "delta", rows), "adopted", limit);
  }
  const range = batch.range;
  const end = transcriptCommittedEnd(state);
  if (end == null) {
    return committed(
      state,
      [...state.segments, { kind: "delta", range, rows: batch.rows }],
      "adopted",
      limit,
    );
  }
  if (range.end <= end) return { state, outcome: "ignored_stale" };
  if (range.start < end) return { state, outcome: "recover_overlap" };
  const outcome: TranscriptOutcome = range.start > end ? "recover_gap" : "adopted";
  return committed(
    state,
    [...state.segments, { kind: "delta", range, rows: batch.rows }],
    outcome,
    limit,
  );
}

function reduceBackfill(
  state: TranscriptState,
  batch: TranscriptBatch,
): TranscriptResult {
  if (!isRange(batch.range)) return { state, outcome: "rejected_range" };
  const range = batch.range;
  const oldest = transcriptBackfillCursor(state);
  if (oldest != null && range.end > oldest) return { state, outcome: "rejected_range" };
  // Paging older never evicts: the bound applies to tail growth only.
  return {
    state: {
      ...state,
      segments: orderedSegments([
        ...state.segments,
        { kind: "backfill", range, rows: batch.rows },
      ]),
      evictedOlder: Math.max(0, state.evictedOlder - batch.rows.length),
    },
    outcome: "adopted",
  };
}

/** The single owner of transcript reconciliation. Byte ranges decide overlap,
 *  adoption and eviction; nothing is inferred from content, timestamps,
 *  provider ids or cross-batch `seq` equality. */
export function reduceTranscript(
  state: TranscriptState,
  batch: TranscriptBatch,
  limit: number = TRANSCRIPT_RETENTION_LIMIT,
): TranscriptResult {
  if (batch.range != null && !isRange(batch.range)) {
    return { state, outcome: "rejected_range" };
  }
  if (batch.rows.length === 0) return { state, outcome: "ignored_empty" };
  if (state.authority != null && !sameAuthority(state.authority, batch.authority)) {
    const rotated = reduceTranscript(
      { ...createTranscriptState(), authority: batch.authority },
      batch,
      limit,
    );
    if (rotated.outcome !== "adopted") return { state, outcome: rotated.outcome };
    return { state: rotated.state, outcome: "rotated" };
  }
  const based: TranscriptState = { ...state, authority: batch.authority };
  if (batch.kind === "snapshot") return reduceSnapshot(based, batch, limit);
  if (batch.kind === "delta") return reduceDelta(based, batch, limit);
  return reduceBackfill(based, batch);
}

export interface TranscriptProjection {
  rows: NormalizedChatMessage[];
  /** Optimistic rows still awaiting their parsed echo. */
  pending: NormalizedChatMessage[];
}

function toEchoes(rows: readonly NormalizedChatMessage[]): OptimisticEcho[] {
  return rows.map((row) => ({
    id: row.id,
    content: row.content,
    sentAtMs: Number(row.timestamp) || 0,
    clientOrder: row.clientOrder,
  }));
}

/** Place an optimistic row before the committed rows that arrived after it, so
 *  a sent message stays above the turn it triggered and never re-orders
 *  committed history. */
function insertionIndex(rows: readonly NormalizedChatMessage[], clientOrder: number | undefined): number {
  if (clientOrder == null) return rows.length;
  let index = rows.length;
  while (index > 0) {
    const order = rows[index - 1]?.clientOrder;
    if (order == null || order <= clientOrder) break;
    index -= 1;
  }
  return index;
}

/** Committed transcript plus the optimistic rows that have no parsed echo yet.
 *  An echoed row reconciles exactly once: it is dropped here and never
 *  re-appended, because matching runs against the whole committed projection. */
export function projectTranscript(
  state: TranscriptState,
  optimistic: readonly NormalizedChatMessage[] = [],
): TranscriptProjection {
  const rows = transcriptRows(state);
  if (optimistic.length === 0) return { rows, pending: [] };
  const parsedUsers = rows.filter((row) => row.type === "user" || row.type === "local_command");
  const { matchedIds } = matchedOptimisticEchoes(toEchoes(optimistic), parsedUsers);
  const matched = new Set(matchedIds);
  const pending = optimistic.filter((row) => !matched.has(row.id));
  if (pending.length === 0) return { rows, pending: [] };
  const projected = [...rows];
  for (const row of pending) {
    projected.splice(insertionIndex(projected, row.clientOrder), 0, row);
  }
  return { rows: projected, pending };
}
