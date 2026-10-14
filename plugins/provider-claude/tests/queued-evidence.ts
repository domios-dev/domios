import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

interface EvidenceRow {
  role: string;
  uuid: string;
  provider_session_id: string;
  timelineMarker: unknown;
  blocks: Array<{ data: { text: string } }>;
}

export function runQueuedEvidenceTests(
  load: () => { parseSessionDelta(chunk: string): { messages: EvidenceRow[] } },
  eq: (actual: unknown, expected: unknown, name: string) => void,
  assert: (condition: boolean, name: string) => void,
): void {
  const fixture = fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)),
    "fixtures", "owner-mid-turn-2.1.296.jsonl"), "utf8");
  const records = fixture.trim().split("\n").map((line) => JSON.parse(line));
  const parsed = load().parseSessionDelta(fixture).messages;
  const users = parsed.filter((row: { role: string }) => row.role === "user");
  eq(users.length, 2, "only consumed own-session attachments provide user evidence, queue operations do not");
  for (const [index, record] of records.filter((record) => record.type === "attachment").entries()) {
    eq(users[index].blocks[0].data.text, record.attachment.prompt, "queued evidence preserves the entire exact payload");
    eq(users[index].uuid, record.uuid, "queued evidence preserves source identity");
    eq(users[index].provider_session_id, record.sessionId, "queued evidence belongs to the source session");
    eq(users[index].timelineMarker, { kind: "queued" }, "consumed input declares its queue marker");
  }
  assert(users[0].blocks[0].data.text.includes("<domios from=\"report\""),
    "a glued routed report is retained, never guessed to be a standalone owner turn");
}
