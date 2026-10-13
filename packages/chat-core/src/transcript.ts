import { projectTranscriptParts } from "./messageParts";
import { placePendingSends } from "./sendPlacement";
import { mergeById } from "./merge";
import { reconcileSends, type ChatSend, type SendEcho } from "./sendOrder";
import type { NormalizedChatMessage } from "./types";

/** Half-open source window; byte boundaries align to complete records. */
export interface TranscriptRange {
  unit?: "bytes" | "items";
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
  /** `null` for events with no paging window. */
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

/** Unranged rows are capped; ranged sources evict whole oldest windows. */
export const TRANSCRIPT_RETENTION_LIMIT = 440;

export function createTranscriptState(): TranscriptState {
  return { authority: null, segments: [], evictedOlder: 0 };
}

function ownedRows(
  authority: TranscriptAuthority,
  rows: readonly NormalizedChatMessage[],
): NormalizedChatMessage[] {
  const sourceSessionId = authority.sourceSessionId ?? undefined;
  return rows.map((row) => (
    row.sourceSessionId != null ? row : { ...row, sourceSessionId }
  ));
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
  return state.segments.filter((segment) => segment.range != null && segment.range.unit !== "items");
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

/** Earliest loaded source position for `chat_backfill`. */
export function transcriptBackfillCursor(state: TranscriptState): number | null {
  let start: number | null = null;
  for (const segment of state.segments) {
    if (segment.range == null) continue;
    const candidate = segment.range;
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

/** Provider IDs survive every source window; the newest occurrence wins. */
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

/** Ranged segments stay whole so the paging cursor cannot skip evicted rows. */
function withRetentionBound(state: TranscriptState, limit: number): TranscriptState {
  let evicted = 0;
  let kept = orderedSegments(state.segments).map((segment) => {
    if (segment.range != null || segment.rows.length <= limit) return segment;
    const overflow = segment.rows.length - limit;
    evicted += overflow;
    return { ...segment, rows: segment.rows.slice(overflow) };
  });
  let total = kept.reduce((count, segment) => count + segment.rows.length, 0);
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

function reduceItems(state: TranscriptState, batch: TranscriptBatch): TranscriptResult {
  const ranges = state.segments.flatMap((segment) => segment.range?.unit === "items" ? [segment.range] : []);
  if (batch.range != null) ranges.push(batch.range);
  const range: TranscriptRange = {
    unit: "items",
    start: Math.min(...ranges.map((entry) => entry.start)),
    end: Math.max(...ranges.map((entry) => entry.end)),
  };
  const previous = transcriptRows(state);
  const incoming = ownedRows(batch.authority, batch.rows);
  const rows = batch.kind === "backfill" ? mergeById(incoming, previous) : mergeById(previous, incoming);
  return { state: { ...state, segments: [{ kind: batch.kind, range, rows }] }, outcome: "adopted" };
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
      { kind: "snapshot", range, rows: ownedRows(batch.authority, batch.rows) },
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
    const rows = mergeById(unrangedRows(state), ownedRows(batch.authority, batch.rows));
    return committed(state, withUnrangedRows(state, "delta", rows), "adopted", limit);
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
  return committed(
    state,
    [...state.segments, { kind: "delta", range, rows: ownedRows(batch.authority, batch.rows) }],
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
        { kind: "backfill", range, rows: ownedRows(batch.authority, batch.rows) },
      ]),
      evictedOlder: Math.max(0, state.evictedOlder - batch.rows.length),
    },
    outcome: "adopted",
  };
}

/** Byte ranges decide window adoption; source IDs decide row identity. */
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
  if (batch.range?.unit === "items" || (batch.range == null && based.segments.some((segment) => segment.range?.unit === "items"))) {
    return reduceItems(based, batch);
  }
  if (batch.kind === "snapshot") return reduceSnapshot(based, batch, limit);
  if (batch.kind === "delta") return reduceDelta(based, batch, limit);
  return reduceBackfill(based, batch);
}

export interface TranscriptProjection {
  rows: NormalizedChatMessage[];
  /** Sends still awaiting their source echo. */
  pending: ChatSend[];
  /** Echo identity of every send reconciled against this transcript. */
  echoes: Map<string, SendEcho>;
}

export function projectTranscript(
  state: TranscriptState,
  sends: readonly ChatSend[] = [],
): TranscriptProjection {
  const committedRows = transcriptRows(state);
  const { echoes, pending } = reconcileSends(sends, committedRows);
  const projectedCommittedRows = projectTranscriptParts(committedRows, sends);
  return { rows: placePendingSends(projectedCommittedRows, pending), pending, echoes };
}
