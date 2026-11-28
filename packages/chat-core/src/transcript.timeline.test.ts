import { expect, test } from "vitest";
import { parseDsBlocks } from "./dsBlocks";
import { envelopeRows, type MessageSegment } from "./envelope";
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

const richBody = 'refinement complete\n\n<ds-page src="C:/fixture/index.html" />';
const richTip = "reply to Forge";
const richWire = `<domios from="tab" label="Forge" tab="sender">${richBody} <tip>${richTip}</tip></domios>`;
const frame: MessageSegment = {
  type: "frame", kind: "tab", from: "tab", attrs: { from: "tab", label: "Forge", tab: "sender" },
  body: richBody, tip: richTip,
};
const richReport = { ...report, content: richBody, inboundKind: "tab" as const, segments: [frame] };
const rawEcho: NormalizedChatMessage = {
  ...report, content: `<pasted_content>\n${richWire.replaceAll("\n", "\r\n")}\n</pasted_content>`,
};

test("partial input stays plain until complete, and stale live and stored copies keep the completed card", () => {
  const prefix = richWire.slice(0, richWire.indexOf("<ds-page") + 8);
  const partial = { ...report, content: prefix, segments: [{ type: "text" as const, text: prefix }] };
  const range = { start: 0, end: 100 };
  const advance = (state: ReturnType<typeof createTranscriptState>, row: NormalizedChatMessage, kind: "snapshot" | "delta") =>
    reduceTranscript(state, { kind, authority, range: kind === "snapshot" ? range : null, rows: [row] }).state;
  let state = advance(createTranscriptState(), partial, "snapshot");
  for (const row of [partial, { ...partial }]) {
    state = advance(state, row, "delta");
    expect(projectTranscript(state, [], "source").rows[0].segments?.[0]?.type).toBe("text");
  }
  for (const [row, kind] of [
    [richReport, "snapshot"], [rawEcho, "delta"], [richReport, "delta"],
    [rawEcho, "snapshot"], [partial, "delta"], [richReport, "snapshot"],
  ] as const) {
    state = advance(state, row, kind);
    const projected = projectTranscript(state, [], "source").rows;
    expect(projected.map(item => item.id)).toEqual([report.id]);
    expect(projected[0].segments).toEqual([frame]);
    const [card] = envelopeRows(projected[0].segments!);
    expect(card.frame?.tip).toBe(richTip);
    expect(parseDsBlocks(card.content).map(part => part.type)).toEqual(["markdown", "block"]);
  }
  const edited = richWire.replace("refinement complete", "quoted body");
  state = advance(state, { ...report, content: edited }, "delta");
  expect(projectTranscript(state, [], "source").rows[0].segments).toBe(undefined);
});

test("a reused ID cannot carry a routed card into a new session or a different send", () => {
  const state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null, rows: [{ ...richReport, correlationId: "original" }],
  }).state;
  const replaced = reduceTranscript(state, {
    kind: "delta", authority, range: null, rows: [{ ...rawEcho, correlationId: "other" }],
  }).state;
  const rotated = reduceTranscript(state, {
    kind: "snapshot", authority: { ...authority, sourceSessionId: "other" }, range: null, rows: [rawEcho],
  }).state;
  for (const next of [replaced, rotated]) {
    expect(projectTranscript(next, [], "source").rows[0].segments).toBe(undefined);
  }
});

test("a ranged echo retains the card when the earlier range is evicted", () => {
  const state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start: 0, end: 100 }, rows: [richReport],
  }).state;
  const next = reduceTranscript(state, {
    kind: "delta", authority, range: { start: 100, end: 200 }, rows: [rawEcho],
  }, 1).state;
  expect(next.evictedOlder).toBe(1);
  const [row] = projectTranscript(next, [], "source").rows;
  expect(row.segments).toEqual([frame]);
});

test("a cold wrapped echo with exact host correlation projects the completed card", () => {
  const state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null, rows: [{ ...rawEcho, correlationId: "trusted" }],
  }).state;
  const result = projectTranscript(state, [
    { id: report.id, correlationId: "trusted", content: richWire, sentAtMs: Date.parse(at(13)),
      flowAtMs: Date.parse(at(13)), segments: [frame] },
  ], "source");
  expect(result.rows).toHaveLength(1);
  expect(result.rows[0].segments).toEqual([{ ...frame, correlationId: "trusted" }]);
});
