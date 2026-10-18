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
}

export type MatchedRowRecord =
  | { kind: "structured"; thread_id: string; item_id: string }
  | { kind: "jsonl"; incarnation: string; record_start: number; record_row: number };

export interface MatchedRow {
  source: { source_session_id: string; record: MatchedRowRecord };
  ordinal: number;
}

export function captureSendEchoBoundary(
  authority: TranscriptAuthority | null, rows: readonly NormalizedChatMessage[],
): SendEchoBoundary | undefined {
  if (!authority?.sourceSessionId) return;
  return { ...authority, sourceSessionId: authority.sourceSessionId, lastRowId: rows[rows.length - 1]?.id ?? null };
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
  echoEligible?: boolean;
  flowAtMs?: number;
  segments?: MessageSegment[];
  echoBoundary?: SendEchoBoundary;
}

function isUserAuthored(row: NormalizedChatMessage): boolean {
  return row.inheritedFromSessionId == null && (row.type === "user" || row.type === "local_command");
}

export function matchesMatchedRow(row: NormalizedChatMessage, matchedRow: MatchedRow): boolean {
  if (!isUserAuthored(row) || row.sourceSessionId !== matchedRow.source.source_session_id) return false;
  const record = matchedRow.source.record;
  return record.kind === "structured"
    && record.thread_id === matchedRow.source.source_session_id
    && (row.id === record.item_id || row.sourceMessageId === record.item_id);
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
  };
}

/** A recorded echo outside the loaded window still hides its reconciled send. */
export function reconcileSends(
  sends: readonly ChatSend[],
  rows: readonly NormalizedChatMessage[],
  authority: TranscriptAuthority | null = null,
): { echoes: Map<string, SendEcho>; pending: ChatSend[] } {
  const echoes = new Map<string, SendEcho>();
  const pending: ChatSend[] = [];
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
  const appendedRow = (sendId: string, index: number) => {
    const boundary = boundaries.get(sendId);
    if (boundary == null) return;
    return index > boundary && rows[index].sourceSessionId === authority?.sourceSessionId;
  };
  for (const send of sends) {
    if (send.echoEligible === false) {
      pending.push(send);
      continue;
    }
    if (send.matchedRow) {
      const matchedRow = send.matchedRow;
      const sourceSessionId = matchedRow.source.source_session_id;
      if (send.echoBoundary
        ? send.echoBoundary.sourceSessionId !== sourceSessionId
        : sourceSessionId !== authority?.sourceSessionId) {
        if (!send.echo) pending.push(send);
        continue;
      }
      const record = matchedRow.source.record;
      const index = users.find((candidate) => {
        if (recorded.has(candidate)) return false;
        const row = rows[candidate];
        if (record.kind === "structured") return matchesMatchedRow(row, matchedRow);
        return send.correlationId != null
          && row.sourceSessionId === sourceSessionId
          && row.correlationId === send.correlationId;
      });
      if (index == null && !send.echo) pending.push(send);
      else if (index != null) {
        recorded.add(index);
        claim(send, index);
      }
      continue;
    }
    if (send.echoBoundary && (send.echoBoundary.sourceSessionId !== authority?.sourceSessionId
      || send.echoBoundary.sessionBindingNonce !== authority?.sessionBindingNonce)) {
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
  if (open.length === 0) return { echoes, pending };
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
  return { echoes, pending };
}
