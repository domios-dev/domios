import { expect, test } from "vitest";
import { createTranscriptState, projectTranscript, reduceTranscript, transcriptRows } from "./transcript";
import { reduceToolEvents } from "./toolEvents";
import type { NormalizedChatMessage } from "./types";

const authority = { sourceSessionId: "thread", sessionBindingNonce: "binding" };
const at = (second: number) => new Date(Date.UTC(2026, 10, 28, 6, 54, second)).toISOString();
const routed = "<domios from=\"tab\" label=\"Forge\" tab=\"sender\">refinement complete</domios>";
const report: NormalizedChatMessage = {
  id: "forge-report", type: "user", content: routed, timestamp: at(13), seq: 4767,
};
const command = (id: string, seq: number, second: number): NormalizedChatMessage => ({
  id, type: "tool_call", content: id, toolId: id, toolKind: "command", seq, timestamp: at(second),
  toolInput: { command: id, commandBatchId: "work" },
});

function timeline(rows: NormalizedChatMessage[]) {
  return reduceToolEvents(rows).map(item => item.kind === "message"
    ? { id: item.message.id, members: [] }
    : { id: item.event.message.id, members: (item.event.commandBatch ?? []).map(member => member.message.id) });
}

test("routed input stays between the same tool rows across live patches, backfill and reload", () => {
  const before = command("before", 4766, 8);
  const after = command("after", 4768, 31);
  const initial = [before, report, after];
  const range = { start: 100, end: 200 };
  const snapshot = reduceTranscript(createTranscriptState(), { kind: "snapshot", authority, range, rows: initial }).state;
  const expected = initial.map(row => ({ id: row.id, members: [] }));
  expect(timeline(projectTranscript(snapshot, [], "source").rows)).toEqual(expected);

  const updatedReport = { ...report, timestamp: at(-18) };
  const result: NormalizedChatMessage = {
    id: "after-result", type: "tool_result", content: "done", toolId: after.id, seq: 4771, timestamp: at(12),
  };
  const live = reduceTranscript(snapshot, {
    kind: "delta", authority, range: null, rows: [result, updatedReport],
  }).state;
  expect(timeline(projectTranscript(live, [], "source").rows)).toEqual(expected);
  expect(transcriptRows(live, "source").find(row => row.id === report.id)?.timestamp).toBe(updatedReport.timestamp);
  const tool = reduceToolEvents(projectTranscript(live, [], "source").rows).at(-1);
  expect(tool?.kind === "tool" && tool.event.result).toBe("done");

  const older: NormalizedChatMessage = { id: "older", type: "assistant", seq: 4765, content: "older" };
  const paged = reduceTranscript(live, {
    kind: "backfill", authority, range: { start: 0, end: range.start }, rows: [older],
  }).state;
  const reloaded = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: range.end }, rows: [older, before, updatedReport, after, result],
  }).state;
  const whole = [{ id: older.id, members: [] }, ...expected];
  expect(timeline(projectTranscript(paged, [], "source").rows)).toEqual(whole);
  expect(timeline(projectTranscript(reloaded, [], "source").rows)).toEqual(whole);
});

test("source ordering keeps the exact provider echo and one held send", () => {
  const before = command("before", 4766, 8);
  const after = command("after", 4768, 31);
  const state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null,
    rows: [before, { ...report, timestamp: at(-18), correlationId: "forge" }, after],
  }).state;
  const projection = projectTranscript(state, [
    { id: "pending-forge", correlationId: "forge", content: routed, sentAtMs: Date.parse(at(13)) },
    { id: "held", content: "next", sentAtMs: Date.parse(at(30)), held: true, neverWritten: true },
  ], "source");
  expect(projection.rows.map(row => row.id)).toEqual([before.id, report.id, after.id, "held"]);
  expect(projection.echoes.has("pending-forge")).toBe(true);
  expect(projection.pending.map(send => send.id)).toEqual(["held"]);
});
