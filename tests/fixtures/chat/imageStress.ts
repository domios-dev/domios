import { readFileSync } from "node:fs";
import path from "node:path";
import { plain } from "../../../plugins/provider-shared/tests/recordHookFixture";
import { parser, flatten } from "./airTranscript";
import type { NormalizedChatMessage } from "../../../packages/chat-core/src/types";

const descriptor = JSON.parse(readFileSync(path.resolve(import.meta.dirname, "image-stress.json"), "utf8")) as {
  sourceSessionId: string; snapshotStart: number; records: Array<{ line: number; start: number; end: number }>;
};
const lines = readFileSync(path.resolve(import.meta.dirname, "image-stress.jsonl"), "utf8").trimEnd().split("\n");
export const stressAuthority = { sourceSessionId: descriptor.sourceSessionId, sessionBindingNonce: "stress-launch" };
export const stressRange = (first: number, last: number) => ({
  start: descriptor.records.find(record => record.line === first)!.start,
  end: descriptor.records.find(record => record.line === last)!.end,
  unit: "bytes" as const,
});
export function stressRows(first: number, last: number) {
  const text = descriptor.records.flatMap((record, index) => record.line >= first && record.line <= last ? [lines[index]] : []).join("\n") + "\n";
  return plain(parser.parseSessionDelta(text).messages).flatMap((row, seq) => flatten([row]).map(message => ({
    ...message, sourceSessionId: stressAuthority.sourceSessionId, seq,
  })));
}
export const stressReplyIds = ["608fc36a-3b42-4918-b4b6-d88bfd1304bb-0", "1ada7c88-22a6-4860-ae17-d9acb0ea73de-0"];

export const stressStart = JSON.parse(readFileSync(path.resolve(import.meta.dirname, "image-stress-start.json"), "utf8")) as {
  kind: "snapshot"; messages: NormalizedChatMessage[]; source_session_id: string; session_binding_nonce: string;
  start_offset: number; offset: number; offset_kind: "bytes"; text: string; pending: boolean;
};
export const stressBackfill = JSON.parse(readFileSync(path.resolve(import.meta.dirname, "image-stress-backfill.json"), "utf8")) as {
  messages: NormalizedChatMessage[]; text: string; start_offset: number; end_offset: number; offset_kind: "bytes";
  source_session_id: string; session_binding_nonce: string;
};
export const stressPages = (JSON.parse(readFileSync(path.resolve(import.meta.dirname, "image-stress-pages.json"), "utf8")) as {
  pages: Array<{ response: typeof stressBackfill }>;
}).pages.map(page => page.response);
