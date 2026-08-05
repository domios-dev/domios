import { matchedOptimisticEchoes, type OptimisticEcho } from "./dedupe";
import { mergeById } from "./merge";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

/** Half-open byte range `[start, end)` of a session source, both ends on a
 *  complete-record boundary. */
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
  /** Arrival ordinals handed to live rows; the only ordering signal an
   *  optimistic row can be placed against. */
  arrivals: number;
}

export interface TranscriptBatch {
  kind: TranscriptBatchKind;
  authority: TranscriptAuthority;
  range: TranscriptRange | null;
  rows: NormalizedChatMessage[];
}

export type TranscriptOutcome =
  | "adopted"
  | "rotated"
  | "ignored_stale"
  | "ignored_empty"
  | "rejected_range"
  | "recover_gap"
  | "recover_overlap";

export interface TranscriptResult {
  state: TranscriptState;
  outcome: TranscriptOutcome;
}

/** Hard bound on retained source rows; whole oldest windows go first. */
export const TRANSCRIPT_RETENTION_LIMIT = 440;

export function createTranscriptState(): TranscriptState {
  return { authority: null, segments: [], evictedOlder: 0, arrivals: 0 };
}

/** Ordinal an optimistic row must carry to sit above every later arrival. */
export function nextOptimisticOrder(state: TranscriptState): number {
  return state.arrivals;
}

/** History rows only inherit the batch session; they never take an ordinal. */
function ownedRows(
  authority: TranscriptAuthority,
  rows: readonly NormalizedChatMessage[],
): NormalizedChatMessage[] {
  const sourceSessionId = authority.sourceSessionId ?? undefined;
  return rows.map((row) => (
    row.sourceSessionId != null ? row : { ...row, sourceSessionId }
  ));
}

/** Live rows take the next arrival ordinals; history never does, so a re-parsed
 *  window cannot push a pending optimistic row upward. */
function arrivedRows(
  state: TranscriptState,
  authority: TranscriptAuthority,
  rows: readonly NormalizedChatMessage[],
): { rows: NormalizedChatMessage[]; arrivals: number } {
  let arrivals = state.arrivals;
  const sourceSessionId = authority.sourceSessionId ?? undefined;
  const stamped = rows.map((row) => {
    const clientOrder = row.clientOrder ?? arrivals;
    if (row.clientOrder == null) arrivals += 1;
    return { ...row, clientOrder, sourceSessionId: row.sourceSessionId ?? sourceSessionId };
  });
  return { rows: stamped, arrivals };
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

/** Transport ids and `seq` are per-batch counters that other batches reuse, so
 *  a ranged row is identified by its own batch range plus its position. */
function rangedRows(
  range: TranscriptRange,
  rows: readonly NormalizedChatMessage[],
): NormalizedChatMessage[] {
  return rows.map((row, ordinal) => {
    const id = `${range.start}:${ordinal}`;
    return row.id === id ? row : { ...row, id };
  });
}

function rangedSegments(state: TranscriptState): TranscriptSegment[] {
  return state.segments.filter((segment) => segment.range != null);
}

/** End of the committed byte span — the live cursor. */
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

/** Segments never overlap, so a repeated id can only come from an unranged
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

/** Whole windows only: slicing rows out of a segment would leave its range
 *  describing rows it no longer holds, stranding them below the paging cursor. */
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
    const rows = mergeById(unrangedRows(state), ownedRows(batch.authority, batch.rows));
    return committed(state, withUnrangedRows(state, "snapshot", rows), "adopted", limit);
  }
  const range = batch.range;
  // Nothing proves which rows of a segment straddling the window end were
  // re-parsed, so a window landing there (a stale late parse) is refused.
  const straddlesEnd = state.segments.some(
    (segment) => segment.range != null
      && segment.range.start < range.end
      && range.end < segment.range.end,
  );
  if (straddlesEnd) return { state, outcome: "rejected_range" };
  // Authoritative for its own window only: older segments, a newer live tail,
  // and rows with no range at all survive it.
  const preserved = state.segments.filter(
    (segment) => segment.range == null
      || segment.range.end <= range.start
      || segment.range.start >= range.end,
  );
  return committed(
    state,
    [
      ...preserved,
      { kind: "snapshot", range, rows: rangedRows(range, ownedRows(batch.authority, batch.rows)) },
    ],
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
    const arrived = arrivedRows(state, batch.authority, batch.rows);
    const rows = mergeById(unrangedRows(state), arrived.rows);
    return committed(
      { ...state, arrivals: arrived.arrivals },
      withUnrangedRows(state, "delta", rows),
      "adopted",
      limit,
    );
  }
  const range = batch.range;
  const end = transcriptCommittedEnd(state);
  // A committed transcript never grows a hole: only a range that continues the
  // committed span is adopted, everything else asks for an authoritative parse.
  if (end != null) {
    if (range.end <= end) return { state, outcome: "ignored_stale" };
    if (range.start < end) return { state, outcome: "recover_overlap" };
    if (range.start > end) return { state, outcome: "recover_gap" };
  }
  const arrived = arrivedRows(state, batch.authority, batch.rows);
  return committed(
    { ...state, arrivals: arrived.arrivals },
    [...state.segments, { kind: "delta", range, rows: rangedRows(range, arrived.rows) }],
    "adopted",
    limit,
  );
}

function reduceBackfill(state: TranscriptState, batch: TranscriptBatch): TranscriptResult {
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
        { kind: "backfill", range, rows: rangedRows(range, ownedRows(batch.authority, batch.rows)) },
      ]),
      evictedOlder: Math.max(0, state.evictedOlder - batch.rows.length),
    },
    outcome: "adopted",
  };
}

/** The single owner of transcript reconciliation: byte ranges decide overlap,
 *  adoption and eviction — never content, timestamps, provider ids or seq. */
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

/** Places a pending optimistic row inline by timestamp instead of pinning it
 *  below later-arriving committed rows; a null stamp falls back to the tail. */
function insertionIndexByTimestamp(
  rows: readonly NormalizedChatMessage[],
  timestamp: string | number | undefined,
): number {
  const ts = timestampMs(timestamp);
  if (ts == null) return rows.length;
  let index = rows.length;
  while (index > 0) {
    const rowTs = timestampMs(rows[index - 1]?.timestamp);
    if (rowTs == null || rowTs <= ts) break;
    index -= 1;
  }
  return index;
}

/** An echoed row reconciles once: matching runs against the whole committed
 *  projection, so a reconciled row is never re-appended. */
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
    projected.splice(insertionIndexByTimestamp(projected, row.timestamp), 0, row);
  }
  return { rows: projected, pending };
}
