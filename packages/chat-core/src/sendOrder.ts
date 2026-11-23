import { matchedOptimisticEchoes } from "./dedupe";
import { normalizeChatText } from "./normalize";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";
import type { MessageSegment } from "./envelope";
import type { TranscriptAuthority } from "./transcript";

export interface SendEchoBoundary {
  sourceSessionId: string;
  sessionBindingNonce: string | null;
  lastRowId: string | null;
  endOffset?: number;
}

export type { SourceRecord as MatchedRowRecord } from "./gen/SourceRecord";
import type { MatchedRow } from "./gen/MatchedRow";
export type { MatchedRow } from "./gen/MatchedRow";

export function captureSendEchoBoundary(
  authority: TranscriptAuthority | null, rows: readonly NormalizedChatMessage[],
  endOffset?: number | null,
): SendEchoBoundary | undefined {
  if (!authority?.sourceSessionId) return;
  return { ...authority, sourceSessionId: authority.sourceSessionId, lastRowId: rows[rows.length - 1]?.id ?? null,
    ...(endOffset != null ? { endOffset } : {}) };
}

/** The committed source row a send reconciled to, identified by what re-parsing keeps stable. */
export interface SendEcho {
  content: string;
  timestampMs: number | null;
}

/** A send the host accepted; `sentAtMs` bounds matching to its source echo. */
export interface ChatSend {
  id: string;
  content: string;
  sentAtMs: number;
  correlationId?: string;
  echo?: SendEcho;
  matchedRow?: MatchedRow;
  delivered?: boolean;
  echoEligible?: boolean;
  /** The host holds it unwritten, so no row can be its echo by text; only its own token can name a row. */
  neverWritten?: boolean;
  flowAtMs?: number;
  /** Held by the host until the running turn ends, so it waits after every committed row. */
  held?: boolean;
  segments?: MessageSegment[];
  echoBoundary?: SendEchoBoundary;
}

function isUserAuthored(row: NormalizedChatMessage): boolean {
  return row.inheritedFromSessionId == null && (row.type === "user" || row.type === "local_command");
}

export function matchesMatchedRow(row: NormalizedChatMessage, matchedRow: MatchedRow): boolean {
  if (!isUserAuthored(row) || row.sourceSessionId !== matchedRow.source.source_session_id) return false;
  const record = matchedRow.source.record;
  if (record.kind === "jsonl") {
    const source = row.sourceRecord;
    return source?.kind === "jsonl" && source.incarnation === record.incarnation
      && source.record_start === record.record_start && source.record_row === record.record_row;
  }
  return record.kind === "structured"
    && record.thread_id === matchedRow.source.source_session_id
    && (row.id === record.item_id || row.sourceMessageId === record.item_id || row.runtimeSourceItemId === record.item_id);
}

/** The row lies where the claimed record's identity is comparable: its session and, for JSONL, its file incarnation. */
function inClaimedRecordSpace(row: NormalizedChatMessage, matchedRow: MatchedRow): boolean {
  if (row.sourceSessionId !== matchedRow.source.source_session_id) return false;
  const record = matchedRow.source.record;
  return record.kind !== "jsonl" || (row.sourceRecord?.kind === "jsonl" && row.sourceRecord.incarnation === record.incarnation);
}

/** An unstamped or regrown-file row of the claimed session cannot contradict the record, so ordinary echo rules may link it. */
export function outsideClaimedRecordSpace(row: NormalizedChatMessage, matchedRow: MatchedRow): boolean {
  return row.sourceSessionId === matchedRow.source.source_session_id && !inClaimedRecordSpace(row, matchedRow);
}

export function claimedRecordLoaded(rows: readonly NormalizedChatMessage[], matchedRow: MatchedRow): boolean {
  return rows.some(row => inClaimedRecordSpace(row, matchedRow));
}

function echoContent(content: string): string {
  return normalizeChatText(content).toLowerCase();
}

function sendEchoOf(row: NormalizedChatMessage): SendEcho {
  return { content: echoContent(row.content), timestampMs: timestampMs(row.timestamp) };
}

function isRecordedEcho(echo: SendEcho, row: NormalizedChatMessage): boolean {
  return echo.timestampMs === timestampMs(row.timestamp) && echo.content === echoContent(row.content);
}

export function sendRow(send: ChatSend): NormalizedChatMessage {
  return {
    id: send.id,
    type: "user",
    content: send.content,
    segments: send.segments,
    timestamp: String(send.sentAtMs),
    correlationId: send.correlationId,
    ...(send.matchedRow ? { sourceSessionId: send.matchedRow.source.source_session_id,
      sourceRecord: send.matchedRow.source.record } : {}),
  };
}

export interface UnloadedHistoryWindow {
  /** Oldest known time (ms) among loaded own-session rows, when any loaded row is timed. */
  startMs: number | null;
  /** Byte start offset of the loaded window when the transcript uses byte paging. */
  byteStart?: number | null;
  /** True when the loaded window has own-session rows, older history is unloaded, and none carries a time. */
  untimed?: boolean;
}

/** An exact claim admitted before the loaded window of its own session names a record in
 *  history the window has not paged in, so it is settled there, never waiting at the tail. */
function claimPrecedesWindow(
  send: ChatSend,
  authority: TranscriptAuthority | null,
  rows: readonly NormalizedChatMessage[],
  window: number | UnloadedHistoryWindow | null,
): boolean {
  if (window == null || send.matchedRow == null || authority?.sourceSessionId == null
    || send.matchedRow.source.source_session_id !== authority.sourceSessionId) return false;
  const record = send.matchedRow.source.record;
  if (typeof window !== "number" && record.kind === "jsonl"
    && window.byteStart != null && record.record_start < window.byteStart) return true;
  const boundary = send.echoBoundary?.sourceSessionId === authority.sourceSessionId ? send.echoBoundary : undefined;
  return sendPrecedesWindow(send, boundary, rows, window);
}

function sendPrecedesWindow(
  send: ChatSend,
  boundary: SendEchoBoundary | undefined,
  rows: readonly NormalizedChatMessage[],
  window: number | UnloadedHistoryWindow,
): boolean {
  if (typeof window === "number") return send.sentAtMs < window;
  if (window.startMs != null) return send.sentAtMs < window.startMs;
  return window.untimed === true
    && (boundary?.lastRowId == null || !rows.some(row => row.id === boundary.lastRowId));
}

/** A recorded echo or exact claim outside the loaded window still hides its reconciled send. */
export function reconcileSends(
  sends: readonly ChatSend[],
  rows: readonly NormalizedChatMessage[],
  authority: TranscriptAuthority | null = null,
  rowStarts: ReadonlyMap<string, number> = new Map(),
  windowStart: number | UnloadedHistoryWindow | null = null,
): { echoes: Map<string, SendEcho>; pending: ChatSend[]; outside: Set<string> } {
  const echoes = new Map<string, SendEcho>();
  const pending: ChatSend[] = [];
  const outside = new Set<string>();
  const claim = (send: ChatSend, index: number) => {
    echoes.set(send.id, sendEchoOf(rows[index]));
  };
  const users = rows.flatMap((row, index) => (isUserAuthored(row) ? [index] : []));
  const recorded = new Set<number>();
  const open: ChatSend[] = [];
  const boundaries = new Map(sends.filter((send) => send.echoBoundary).map((send) => {
    const lastRowId = send.echoBoundary!.lastRowId;
    const index = lastRowId == null ? -1 : rows.findIndex((row) => row.id === lastRowId);
    return [send.id, lastRowId != null && index < 0 ? rows.length : index];
  }));
  const sendsById = new Map(sends.map(send => [send.id, send]));
  const appendedRow = (sendId: string, index: number) => {
    const send = sendsById.get(sendId)!;
    const row = rows[index];
    if (send.matchedRow && !outsideClaimedRecordSpace(row, send.matchedRow)) return false;
    if (send.echoBoundary && row.sourceSessionId === authority?.sourceSessionId) {
      if (send.correlationId != null && row.correlationId === send.correlationId) return true;
      const start = rowStarts.get(row.id);
      if (send.echoBoundary.sessionBindingNonce === authority?.sessionBindingNonce) {
        if (send.echoBoundary.endOffset != null && start != null && start >= send.echoBoundary.endOffset) return true;
      } else if (send.echoBoundary.lastRowId == null) return false;
    }
    const boundary = boundaries.get(sendId);
    if (boundary == null) return;
    return index > boundary && row.sourceSessionId === authority?.sourceSessionId;
  };
  for (const send of sends) {
    if (send.echoEligible === false) {
      pending.push(send);
      continue;
    }
    if (send.neverWritten) {
      const index = send.correlationId == null ? undefined
        : users.find((candidate) => !recorded.has(candidate) && rows[candidate].correlationId === send.correlationId);
      if (index == null) pending.push(send);
      else {
        recorded.add(index);
        claim(send, index);
      }
      continue;
    }
    if (send.matchedRow) {
      const index = users.find(candidate => matchesMatchedRow(rows[candidate], send.matchedRow!));
      if (index != null) {
        recorded.add(index);
        claim(send, index);
        continue;
      }
      if (!users.some(candidate => outsideClaimedRecordSpace(rows[candidate], send.matchedRow!))) {
        if (claimPrecedesWindow(send, authority, rows, windowStart)) outside.add(send.id);
        else if (!send.echo) pending.push(send);
        continue;
      }
    }
    const foreignBoundary = send.echoBoundary != null && send.echoBoundary.sourceSessionId !== authority?.sourceSessionId;
    const deliveredWithoutRow = send.delivered === true && send.matchedRow == null && !foreignBoundary;
    if (deliveredWithoutRow) {
      const session = authority?.sourceSessionId;
      const index = session == null ? undefined : users.find(candidate => {
        const row = rows[candidate];
        return !recorded.has(candidate) && row.sourceSessionId === session
          && (row.id === send.id || (send.correlationId != null && row.correlationId === send.correlationId));
      });
      if (index != null) {
        recorded.add(index);
        claim(send, index);
        continue;
      }
      if (windowStart != null && sendPrecedesWindow(send, send.echoBoundary, rows, windowStart)) {
        outside.add(send.id);
        continue;
      }
    }
    if (foreignBoundary || (send.echoBoundary && !deliveredWithoutRow
      && send.echoBoundary.sessionBindingNonce !== authority?.sessionBindingNonce)) {
      if (!send.echo) pending.push(send);
      continue;
    }
    if (!send.echo) {
      open.push(send);
      continue;
    }
    const echo = send.echo;
    const index = users.find((candidate) => appendedRow(send.id, candidate) !== false && isRecordedEcho(echo, rows[candidate]));
    if (index == null) continue;
    recorded.add(index);
    claim(send, index);
  }
  if (open.length === 0) return { echoes, pending, outside };
  const candidates = users.filter((index) => !recorded.has(index));
  const { claims, pendingIds } = matchedOptimisticEchoes(
    open.map((send) => ({
      id: send.id,
      content: send.content,
      sentAtMs: send.sentAtMs,
      correlationId: send.correlationId,
    })),
    candidates.map((index) => rows[index]),
    (sendId, index) => appendedRow(sendId, candidates[index]),
  );
  const byId = new Map(open.map((send) => [send.id, send]));
  for (const { echoId, parsedIndex } of claims) {
    const send = byId.get(echoId);
    if (send) claim(send, candidates[parsedIndex]);
  }
  const waiting = new Set(pendingIds);
  for (const send of open) if (waiting.has(send.id)) pending.push(send);
  return { echoes, pending, outside };
}
