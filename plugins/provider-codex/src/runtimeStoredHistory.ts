import type { ChatMsg, ParsedSessionRecord } from "@codeterm/plugin-sdk";
import { boundHistoryRows, historyItem } from "./runtimeHistoryBounds";
import { json, runtimeItems } from "./runtimeMessages";
import { object, text } from "./runtimeProtocol";
import { durationMilliseconds } from "./activity";
import { commandText } from "./execWrapper";

export function storedHistoryAvailable(sessionId: string, head: string | null): boolean {
  if (!sessionId || !head) return false;
  try {
    const record = object(JSON.parse(head.split("\n", 1)[0]));
    const payload = object(record.payload);
    return record.type === "session_meta" && payload.id === sessionId && payload.history_base == null;
  } catch {
    return false;
  }
}

export function projectHistoryItem(raw: unknown, seq: number, turn: string, timestamp: unknown, parent?: string): ChatMsg[] {
  const source = object(raw);
  const rows:ChatMsg[] = source.historyTruncated === true
    ? [{id:String(source.id),seq,type:"system",content:"[content truncated: history item exceeds display budget]"}]
    : boundHistoryRows(runtimeItems(historyItem(raw), seq));
  for (const row of rows) {
    if (parent && source.senderThreadId === parent && source.type === "collabAgentToolCall" && source.tool === "spawnAgent" && row.toolKind === "agent") {
      row.toolInput = json({...object(row.toolInput), agent_parent_thread_id: parent});
    }
    row.toolFlowId = turn;
    if (typeof timestamp === "number" && Number.isFinite(timestamp)) row.timestamp = new Date(timestamp).toISOString();
  }
  return rows;
}

function recordedItem(raw: unknown): Record<string, unknown> {
  const item = Object.fromEntries(Object.entries(object(raw)).map(([key, value]) => [
    key.replace(/_([a-z])/g, (_match, letter: string) => letter.toUpperCase()), value,
  ]));
  if (typeof item.type === "string") item.type = item.type.replace(/^./, letter => letter.toLowerCase());
  if (item.type === "commandExecution") {
    item.command = commandText(item.command);
    item.aggregatedOutput ??= [item.stdout, item.stderr].filter(value => typeof value === "string" && value).join("\n");
    item.durationMs ??= durationMilliseconds({duration:item.duration, duration_ms:item.durationMs});
  }
  if (Array.isArray(item.changes)) item.changes = item.changes.map(change => {
    const value = object(change), kind = object(value.kind);
    return {...value, kind:{...kind, ...(typeof kind.type === "string" ? {type:kind.type.toLowerCase()} : {})}};
  });
  if (Array.isArray(item.content)) item.content = item.content.map(part => {
    const value = object(part);
    return {...value, ...(typeof value.type === "string" ? {type:value.type.replace(/^./, letter => letter.toLowerCase())} : {})};
  });
  if (item.type === "agentMessage" && typeof item.text !== "string" && Array.isArray(item.content)) {
    item.text = item.content.map(object).filter(part => part.type === "text" && typeof part.text === "string").map(part => part.text).join("\n");
  }
  return item;
}

export function storedRecordType(line: string): string | undefined {
  const tokens = /"type"\s*:\s*"([^"\\]*)"|"(?:\\.|[^"\\])*"|[\{\[]/g;
  const opening = tokens.exec(line);
  if (opening?.[0] !== "{") return undefined;
  for (let token = tokens.exec(line); token; token = tokens.exec(line)) {
    if (token[1] !== undefined) return token[1];
    if (token[0] === "{" || token[0] === "[") return undefined;
  }
  return undefined;
}

export function runtimeStoredHistory(sessionId: string, records: string): ChatMsg[] {
  return projectStoredRecords(sessionId, textRecords(records));
}

function* textRecords(records: string): Generator<unknown> {
  for (const line of records.split("\n")) {
    if (!line.trim()) continue;
    const type = storedRecordType(line);
    if (type !== undefined && type !== "event_msg" && type !== "session_meta") continue;
    yield JSON.parse(line);
  }
}

export function runtimeStoredRecords(sessionId: string, records: readonly ParsedSessionRecord[]): ChatMsg[] {
  return projectStoredRecords(sessionId, records.map(record => {
    if (!record.oversized) return record.value;
    const value = object(record.value), payload = object(value.payload);
    return {...value,payload:{...payload,item:{...object(payload.item),historyTruncated:true}}};
  }));
}

function projectStoredRecords(sessionId: string, records: Iterable<unknown>): ChatMsg[] {
  const rows = new Map<string, ChatMsg>();
  for (const raw of records) {
    const record = object(raw);
    const payload = object(record.payload);
    if (record.type === "session_meta" && text(payload.id) !== sessionId) throw new Error("Stored session identity differs");
    if (record.type !== "event_msg" || payload.type !== "item_completed" || payload.thread_id !== sessionId) continue;
    const turn = text(payload.turn_id);
    if (!turn || !text(object(payload.item).id)) throw new Error("Stored runtime item identity missing");
    const projected = projectHistoryItem(recordedItem(payload.item), rows.size, turn, payload.started_at_ms ?? payload.completed_at_ms, sessionId);
    for (const row of projected) {
      row.seq = rows.get(row.id)?.seq ?? rows.size;
      rows.set(row.id, row);
    }
  }
  return [...rows.values()];
}
