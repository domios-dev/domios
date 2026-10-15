import type { ChatMsg } from "@codeterm/plugin-sdk";
import { boundHistoryRows, historyItem } from "./runtimeHistoryBounds";
import { runtimeItems } from "./runtimeMessages";
import { object, text } from "./runtimeProtocol";
import { durationMilliseconds } from "./activity";
import { commandText } from "./execWrapper";

export function projectHistoryItem(raw: unknown, seq: number, turn: string, timestamp: unknown): ChatMsg[] {
  const rows = boundHistoryRows(runtimeItems(historyItem(raw), seq));
  for (const row of rows) {
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
  return item;
}

export function runtimeStoredHistory(sessionId: string, records: string): ChatMsg[] {
  const rows = new Map<string, ChatMsg>();
  for (const line of records.split("\n")) {
    if (!line.trim()) continue;
    const record = object(JSON.parse(line));
    const payload = object(record.payload);
    if (record.type === "session_meta" && text(payload.id) !== sessionId) throw new Error("Stored session identity differs");
    if (record.type !== "event_msg" || payload.type !== "item_completed" || payload.thread_id !== sessionId) continue;
    const turn = text(payload.turn_id);
    if (!turn || !text(object(payload.item).id)) throw new Error("Stored runtime item identity missing");
    const projected = projectHistoryItem(recordedItem(payload.item), rows.size, turn, payload.started_at_ms ?? payload.completed_at_ms);
    for (const row of projected) {
      row.seq = rows.get(row.id)?.seq ?? rows.size;
      rows.set(row.id, row);
    }
  }
  return [...rows.values()];
}
