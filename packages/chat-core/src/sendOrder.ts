import { matchedOptimisticEchoes } from "./dedupe";
import { normalizeChatText } from "./normalize";
import { timestampMs } from "./time";
import type { NormalizedChatMessage } from "./types";

/** The committed source row a send reconciled to, identified by what re-parsing keeps stable. */
export interface SendEcho {
  content: string;
  timestampMs: number | null;
}

/** A send the host accepted; `sentAtMs` is its order key, captured once and never replaced. */
export interface ChatSend {
  id: string;
  content: string;
  sentAtMs: number;
  correlationId?: string;
  echo?: SendEcho;
}

function isUserAuthored(row: NormalizedChatMessage): boolean {
  return row.type === "user" || row.type === "local_command";
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
    timestamp: String(send.sentAtMs),
    correlationId: send.correlationId,
  };
}

/** Maps each echoing row index to its earliest send key. A reconciled send finds its echo by
 *  recorded identity, never content; a recorded echo that is not loaded hides the send. */
export function reconcileSends(
  sends: readonly ChatSend[],
  rows: readonly NormalizedChatMessage[],
): { keys: Map<number, number>; echoes: Map<string, SendEcho>; pending: ChatSend[] } {
  const keys = new Map<number, number>();
  const echoes = new Map<string, SendEcho>();
  const pending: ChatSend[] = [];
  const claim = (send: ChatSend, index: number) => {
    keys.set(index, Math.min(keys.get(index) ?? send.sentAtMs, send.sentAtMs));
    echoes.set(send.id, sendEchoOf(rows[index]));
  };
  const users = rows.flatMap((row, index) => (isUserAuthored(row) ? [index] : []));
  const recorded = new Set<number>();
  const open: ChatSend[] = [];
  for (const send of sends) {
    if (!send.echo) {
      open.push(send);
      continue;
    }
    const echo = send.echo;
    const index = users.find((candidate) => isRecordedEcho(echo, rows[candidate]));
    if (index == null) continue;
    recorded.add(index);
    claim(send, index);
  }
  if (open.length === 0) return { keys, echoes, pending };
  const candidates = users.filter((index) => !recorded.has(index));
  const { claims, pendingIds } = matchedOptimisticEchoes(
    open.map((send) => ({
      id: send.id,
      content: send.content,
      sentAtMs: send.sentAtMs,
      correlationId: send.correlationId,
    })),
    candidates.map((index) => rows[index]),
  );
  const byId = new Map(open.map((send) => [send.id, send]));
  for (const { echoId, parsedIndex } of claims) {
    const send = byId.get(echoId);
    if (send) claim(send, candidates[parsedIndex]);
  }
  const waiting = new Set(pendingIds);
  for (const send of open) if (waiting.has(send.id)) pending.push(send);
  return { keys, echoes, pending };
}
