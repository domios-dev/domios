import { readFileSync } from "node:fs";
import path from "node:path";
import { loadRecordPlugin, plain } from "../../../plugins/provider-shared/tests/recordHookFixture";
import type { NormalizedChatMessage } from "../../../packages/chat-core/src/types";
interface RichRow {
  uuid: string | null;
  role: string;
  timestamp: string | null;
  recordIndex: number;
  timelineMarker?: NormalizedChatMessage["timelineMarker"];
  blocks: Array<{ kind: string; data: Record<string, unknown> }>;
}
interface Parser {
  parseSessionDelta(text: string): { messages: RichRow[] };
  parseSessionRecords(records: unknown[], meta: unknown): { messages: RichRow[] };
}

const descriptor = JSON.parse(readFileSync(path.resolve(import.meta.dirname, "air-transcript.json"), "utf8")) as { pluginDirectory: string; transcript: string };
const parser = loadRecordPlugin(path.resolve(import.meta.dirname, "../../../plugins", descriptor.pluginDirectory)) as unknown as Parser;
const raw = readFileSync(path.resolve(import.meta.dirname, descriptor.transcript), "utf8");
const lines = raw.trimEnd().split("\n");
const authority = { sourceSessionId: "ed30e894-a129-4a03-8550-cb10cdc26149", sessionBindingNonce: "air-launch" };
const noticeId = "0b94af20-b836-4da2-8ab4-6b5a77924d5c-0";
const ownerId = "d157e783-4579-4174-9c97-80a60ef44845-0";

function flatten(rows: RichRow[]): NormalizedChatMessage[] {
  return rows.flatMap(row => row.blocks.flatMap((block, index): NormalizedChatMessage[] => {
    const base = { id: `${row.uuid}-${index}`, timestamp: row.timestamp ?? undefined,
      sourceSessionId: authority.sourceSessionId, timelineMarker: row.timelineMarker };
    if (block.kind === "text") return [{ ...base, type: row.role as NormalizedChatMessage["type"], content: String(block.data.text ?? "").trim() }];
    if (block.kind === "thinking") return [{ ...base, type: "thinking", content: String(block.data.thinking ?? "") }];
    if (block.kind === "tool_use") return [{ ...base, type: "tool_call", content: String(block.data.name ?? ""), toolId: String(block.data.id) }];
    if (block.kind === "tool_result") return [{ ...base, id: `result-${block.data.tool_use_id}`, type: "tool_result", content: "", toolId: String(block.data.tool_use_id) }];
    return [];
  }));
}

const parsed = plain(parser.parseSessionDelta(raw).messages);
const rows = flatten(parsed);
const end = Buffer.byteLength(raw);
const owner = rows.find(row => row.id === ownerId)!;


export { parser, raw, lines, authority, noticeId, ownerId, flatten, parsed, rows, end, owner };
