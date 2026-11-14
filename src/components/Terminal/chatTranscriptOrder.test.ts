import assert from "node:assert/strict";
import { test } from "node:test";
import {
  createTranscriptState,
  projectTranscript,
  reduceTranscript,
  timestampMs,
  transcriptBackfillCursor,
  type NormalizedChatMessage,
  type TranscriptState,
} from "@codeterm/chat-core";
import { placeChatRails } from "./chatBodyVisibility";
import { deliveryProjectionSends, type DeliverySend } from "./chatDeliveryModel";
import { createDeliveryProjectionMemory, queuedDeliveryIds, stableDeliveryEntries, type DeliveryProjectionMemory } from "./chatDeliveryProjection";
import { conversationEntries } from "./chatPagingModel";

// A long-running agent tab: three days of turns, each with the same report tool
// call, read newest page first and paged back the way the Chat view pages it.
const authority = { sourceSessionId: "session", sessionBindingNonce: "launch" };
const START = Date.UTC(2026, 10, 11, 8);
const TURNS = 60;
const TURN_MS = 70 * 60_000;
const ROWS_PER_TURN = 4;
const PAGE = 40;
const WINDOW = 24;

const turnAt = (turn: number) => START + turn * TURN_MS;
const iso = (ms: number) => new Date(ms).toISOString();

function turnRows(turn: number): NormalizedChatMessage[] {
  const at = turnAt(turn);
  return [
    { id: `user-${turn}`, type: "user", content: `opaque-request-${turn}`, timestamp: iso(at) },
    { id: `call-${turn}`, type: "tool_call", content: "", toolName: "Bash", toolKind: "command", toolId: `tool-${turn}`,
      toolArgs: "domios agent report --status in_review --from-pad report.md", timestamp: iso(at + 10_000) },
    { id: `result-${turn}`, type: "tool_result", content: "reported", toolId: `tool-${turn}`, timestamp: iso(at + 12_000) },
    { id: `reply-${turn}`, type: "assistant", content: `opaque-reply-${turn}`, timestamp: iso(at + 20_000) },
  ];
}

const allRows = (turns: number) => Array.from({ length: turns }, (_, turn) => turnRows(turn)).flat();

/** One accepted report per turn, anchored on that turn's tool call and stamped just after it. */
const reports = Array.from({ length: TURNS }, (_, turn) => ({
  at: turnAt(turn) + 11_000,
  messageId: `call-${turn}`,
}));

function snapshot(rows: NormalizedChatMessage[]): TranscriptState {
  const start = Math.max(0, rows.length - PAGE);
  return reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: { start, end: rows.length, unit: "items" }, rows: rows.slice(start),
  }).state;
}

function backfill(state: TranscriptState, rows: NormalizedChatMessage[]): TranscriptState {
  const end = transcriptBackfillCursor(state)!;
  const start = Math.max(0, end - PAGE);
  return reduceTranscript(state, { kind: "backfill", authority, range: { start, end, unit: "items" }, rows: rows.slice(start, end) }).state;
}

interface ViewInput {
  state: TranscriptState;
  visible: number;
  sends?: readonly DeliverySend[];
  turnRunning?: boolean;
  memory?: DeliveryProjectionMemory;
}

const memory = () => createDeliveryProjectionMemory("session");

/** The rendered column, top to bottom, through the same models TabSessionChat composes. */
function renderedColumn({ state, visible, sends = [], turnRunning = false, memory: projectionMemory = memory() }: ViewInput) {
  const liveSends = deliveryProjectionSends(sends, null, queuedDeliveryIds(sends, turnRunning, projectionMemory));
  const projection = projectTranscript(state, liveSends);
  const entries = stableDeliveryEntries(conversationEntries(projection.rows), liveSends, projection.echoes, projectionMemory);
  const start = Math.max(0, entries.length - visible);
  const window = entries.slice(start);
  const placed = placeChatRails({
    visible: true,
    hiddenBefore: start > 0 || (transcriptBackfillCursor(state) ?? 0) > 0,
    hiddenAfter: false,
    messages: window,
    messageTimeMs: ({ msg }) => timestampMs(msg.timestamp),
    reportTimes: reports.map(report => report.at),
    reportPositions: {
      sourceMessageIds: projection.rows.map(msg => msg.sourceMessageId ?? msg.id),
      messageId: ({ msg }) => msg.sourceMessageId ?? msg.id,
      reportMessageIds: reports.map(report => report.messageId),
    },
    askTimes: [],
  });
  const column: Array<{ key: string; at: number }> = [];
  const pushReports = (indices: readonly number[] | undefined) => {
    for (const index of indices ?? []) column.push({ key: `report-${index}`, at: reports[index].at });
  };
  pushReports(placed.reports.afterIndex.get(-1));
  window.forEach(({ msg }, index) => {
    const send = sends.find(candidate => candidate.id === msg.id);
    column.push({ key: msg.id, at: send?.sentAtMs ?? timestampMs(msg.timestamp)! });
    pushReports(placed.reports.afterIndex.get(index));
  });
  pushReports(placed.reports.trailing);
  return column;
}

function assertChronological(column: ReadonlyArray<{ key: string; at: number }>, label: string) {
  const keys = column.map(item => item.key);
  assert.equal(new Set(keys).size, keys.length, `${label}: no row or report appears twice`);
  for (let index = 1; index < column.length; index++) {
    assert.ok(column[index - 1].at <= column[index].at,
      `${label}: ${column[index - 1].key} renders above the older ${column[index].key}`);
  }
}

/** Loading older history may only add rows above what was already on screen. */
function assertPrependOnly(before: ReadonlyArray<{ key: string }>, after: ReadonlyArray<{ key: string }>, label: string) {
  const tail = after.slice(after.length - before.length).map(item => item.key);
  assert.deepEqual(tail, before.map(item => item.key), `${label}: rows already on screen kept their order and neighbours`);
}

function scrollToTop(rows: NormalizedChatMessage[]) {
  let state = snapshot(rows);
  let visible = WINDOW;
  const steps = [renderedColumn({ state, visible })];
  while ((transcriptBackfillCursor(state) ?? 0) > 0 || steps[steps.length - 1].length < rows.length) {
    if ((transcriptBackfillCursor(state) ?? 0) > 0) state = backfill(state, rows);
    visible += WINDOW;
    steps.push(renderedColumn({ state, visible }));
    if (steps.length > 40) throw new Error("paging never reached the start");
  }
  return { state, steps };
}

test("report cards older than the loaded page stay out of view instead of stacking above it", () => {
  const first = renderedColumn({ state: snapshot(allRows(TURNS)), visible: WINDOW });
  const shownTurns = new Set(first.filter(item => item.key.startsWith("call-")).map(item => item.key.slice("call-".length)));
  const shownReports = first.filter(item => item.key.startsWith("report-")).map(item => item.key.slice("report-".length));
  assert.ok(shownReports.length > 0, "the visible turns show their reports");
  assert.deepEqual(shownReports.filter(turn => !shownTurns.has(turn)), [], "only reports of visible turns render");
  assertChronological(first, "first page");
});

test("scrolling to the top pages in strict chronological order without duplicates or regrouping", () => {
  const rows = allRows(TURNS);
  const { steps } = scrollToTop(rows);
  steps.forEach((column, index) => assertChronological(column, `page ${index}`));
  for (let index = 1; index < steps.length; index++) assertPrependOnly(steps[index - 1], steps[index], `page ${index}`);
  const final = steps[steps.length - 1];
  assert.equal(final.filter(item => item.key.startsWith("report-")).length, TURNS, "every report renders exactly once at the top");
  assert.equal(final.filter(item => item.key.startsWith("call-")).length, TURNS, "identical tool calls stay distinct rows");
  for (let turn = 0; turn < TURNS; turn++) {
    const call = final.findIndex(item => item.key === `call-${turn}`);
    assert.equal(final[call + 1]?.key, `report-${turn}`, `report ${turn} follows its own tool call`);
  }
});

test("a reload with newer turns pages back to the same order", () => {
  const before = scrollToTop(allRows(TURNS - 2)).steps.at(-1)!;
  const after = scrollToTop(allRows(TURNS)).steps.at(-1)!;
  assertChronological(after, "after reload");
  const reloadedPrefix = after.filter(item => before.some(old => old.key === item.key));
  assert.deepEqual(reloadedPrefix.map(item => item.key), before.map(item => item.key), "history reads the same after reload");
});

const status = (state: string, sequence = 1) => ({ tab_id: "tab", send_id: `tx-${sequence}`, correlationId: "", state, sequence,
  ledger_id: "ledger", retained_from: 1, error: state === "failed" ? "opaque-failure" : null }) as unknown as DeliverySend["status"];

function send(id: string, sentAtMs: number, state?: string): DeliverySend {
  const base: DeliverySend = { id, correlationId: id, content: `opaque-${id}`, sentAtMs };
  return state ? { ...base, status: { ...status(state), correlationId: id } as DeliverySend["status"] } : base;
}

function neighbours(column: ReadonlyArray<{ key: string }>, key: string) {
  const index = column.findIndex(item => item.key === key);
  assert.ok(index >= 0, `${key} renders`);
  return [column[index - 1]?.key, column[index + 1]?.key];
}

test("after a reload during a running turn, undelivered sends sit at their send time", () => {
  const rows = allRows(TURNS);
  const sends = [send("failed", turnAt(50) + 15_000, "failed"), send("unconfirmed", turnAt(55) + 15_000, "submit_unconfirmed")];
  const column = renderedColumn({ state: snapshot(rows), visible: rows.length, sends, turnRunning: true });
  assertChronological(column, "reloaded column");
  assert.deepEqual(neighbours(column, "failed"), ["report-50", "reply-50"]);
  assert.deepEqual(neighbours(column, "unconfirmed"), ["report-55", "reply-55"]);
});

test("a send queued behind the running turn waits at the tail, then settles at its send time", () => {
  const rows = allRows(TURNS);
  const state = snapshot(rows);
  const sentAt = turnAt(TURNS - 1) + 15_000;
  const projection = memory();
  const view = (sends: DeliverySend[], turnRunning: boolean) =>
    renderedColumn({ state, visible: PAGE, sends, turnRunning, memory: projection });
  assert.equal(view([send("queued", sentAt)], true).at(-1)?.key, "queued", "a fresh send waits after the running output");
  assert.equal(view([send("queued", sentAt, "written")], true).at(-1)?.key, "queued", "written input keeps waiting with its turn");
  const unconfirmed = view([send("queued", sentAt, "submit_unconfirmed")], false);
  assert.deepEqual(neighbours(unconfirmed, "queued"), [`report-${TURNS - 1}`, `reply-${TURNS - 1}`],
    "once the turn is over, an unconfirmed send sits where it was sent");
  assertChronological(unconfirmed, "settled");
  assert.deepEqual(view([send("queued", sentAt, "submit_unconfirmed")], true), unconfirmed,
    "a released send keeps its bubble and slot when the next turn starts");
  const failed = view([send("queued", sentAt, "failed")], true);
  assert.deepEqual(neighbours(failed, "queued"), [`report-${TURNS - 1}`, `reply-${TURNS - 1}`], "a failed send stays at its send time");
});

test("a send that fails while queued moves to its send time once", () => {
  const state = snapshot(allRows(TURNS));
  const sentAt = turnAt(TURNS - 1) + 15_000;
  const projection = memory();
  const view = (sends: DeliverySend[]) => renderedColumn({ state, visible: PAGE, sends, turnRunning: true, memory: projection });
  assert.equal(view([send("queued", sentAt, "held")]).at(-1)?.key, "queued");
  const failed = view([send("queued", sentAt, "failed")]);
  assert.deepEqual(neighbours(failed, "queued"), [`report-${TURNS - 1}`, `reply-${TURNS - 1}`]);
  assert.deepEqual(view([send("queued", sentAt, "failed")]), failed, "the failed bubble stays where it was sent");
});
