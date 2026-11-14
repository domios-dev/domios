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
import type { OutboundReportEntry } from "./ChatTranscriptRow";
import { outboundReportRows, type LiveOutboundReport } from "./outboundReportRows";

// A long-running agent tab: three days of turns, each with the same report tool
// call, read newest page first and paged back the way the Chat view pages it.
const authority = { sourceSessionId: "session", sessionBindingNonce: "launch" };
const START = Date.UTC(2026, 10, 11, 8);
const TURNS = 60;
const TURN_MS = 70 * 60_000;
const PAGE = 40;
const WINDOW = 24;

const turnAt = (turn: number) => START + turn * TURN_MS;
const iso = (ms: number) => new Date(ms).toISOString();

const REPORT_COMMAND = "domios agent report --status in_review --from-pad report.md";

/** What the CLI prints for an accepted report: the daemon receipt, then the delivered body. */
function receipt(turn: number): string {
  return [
    "  accepted        true",
    "  ok              true",
    `  report_id       rep-${turn}`,
    "  report_outcome  (none)",
    "  state           recorded",
    "  target_tab_id   lens0001",
    '  task_report     {"persisted":true,"plan_id":"pl_fixture","report_outcome":null,"task_id":"task_fixture"}',
    `CODETERM_RESOLVED_TEXT=${JSON.stringify(`opaque-report-${turn}`)}`,
  ].join("\n");
}

function turnRows(turn: number, finished = true): NormalizedChatMessage[] {
  const at = turnAt(turn);
  return [
    { id: `user-${turn}`, type: "user", content: `opaque-request-${turn}`, timestamp: iso(at) },
    { id: `call-${turn}`, type: "tool_call", content: "", toolName: "Bash", toolKind: "command", toolId: `tool-${turn}`,
      toolInput: { command: REPORT_COMMAND }, toolArgs: REPORT_COMMAND, timestamp: iso(at + 10_000) },
    ...(finished ? [
      { id: `result-${turn}`, type: "tool_result" as const, content: receipt(turn), toolId: `tool-${turn}`, timestamp: iso(at + 12_000) },
      { id: `reply-${turn}`, type: "assistant" as const, content: `opaque-reply-${turn}`, timestamp: iso(at + 20_000) },
    ] : []),
  ];
}

const allRows = (turns: number) => Array.from({ length: turns }, (_, turn) => turnRows(turn)).flat();

/** The accepted-report event this client saw live for a turn, anchored on that turn's call. */
function liveReceipt(turn: number): LiveOutboundReport {
  return {
    targetTabId: "lens0001",
    at: turnAt(turn) + 11_000,
    position: { source_session_id: authority.sourceSessionId, message_id: `call-${turn}` },
    descriptor: { reportId: `rep-${turn}`, recipient: "Lens", task: "pl_fixture/task_fixture", status: "in_review", sha: null,
      message: `opaque-report-${turn}`, taskOutcome: { persisted: true, transitionRejected: false, cause: null, reportOutcome: null } },
  };
}

const liveStore = (turns: Iterable<number>) => [...turns].map(liveReceipt);
const range = (from: number, to: number) => Array.from({ length: to - from }, (_, index) => from + index);
const FULL_STORE = liveStore(range(0, TURNS));

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
  live?: readonly LiveOutboundReport[];
  sends?: readonly DeliverySend[];
  turnRunning?: boolean;
  memory?: DeliveryProjectionMemory;
}

const memory = () => createDeliveryProjectionMemory("session");

/** The rendered column, top to bottom, through the same models TabSessionChat composes. */
function renderedColumn({ state, visible, live = FULL_STORE, sends = [], turnRunning = false, memory: projectionMemory = memory() }: ViewInput) {
  const liveSends = deliveryProjectionSends(sends, null, queuedDeliveryIds(sends, turnRunning, projectionMemory));
  const projection = projectTranscript(state, liveSends);
  const entries = stableDeliveryEntries(conversationEntries(projection.rows), liveSends, projection.echoes, projectionMemory);
  const start = Math.max(0, entries.length - visible);
  const window = entries.slice(start);
  const cards = outboundReportRows({ rows: window, transcript: projection.rows, live, recipient: (tab) => tab ?? "parent" });
  const placed = placeChatRails({
    visible: true,
    hiddenBefore: start > 0 || (transcriptBackfillCursor(state) ?? 0) > 0,
    hiddenAfter: false,
    messages: window,
    messageTimeMs: ({ msg }) => timestampMs(msg.timestamp),
    reportTimes: cards.rail.map(report => report.at),
    reportPositions: {
      sourceMessageIds: projection.rows.map(msg => msg.sourceMessageId ?? msg.id),
      messageId: ({ msg }) => msg.sourceMessageId ?? msg.id,
      reportMessageIds: cards.rail.map(report => report.position?.message_id ?? null),
    },
    askTimes: [],
  });
  const column: Array<{ key: string; at: number }> = [];
  const pushCard = (card: OutboundReportEntry) =>
    column.push({ key: card.descriptor.reportId.replace("rep-", "report-"), at: card.at! });
  const pushRail = (indices: readonly number[] | undefined) => {
    for (const index of indices ?? []) pushCard(cards.rail[index]);
  };
  pushRail(placed.reports.afterIndex.get(-1));
  window.forEach(({ msg }, index) => {
    const send = sends.find(candidate => candidate.id === msg.id);
    column.push({ key: msg.id, at: send?.sentAtMs ?? timestampMs(msg.timestamp)! });
    for (const card of cards.byRow.get(index) ?? []) pushCard(card);
    pushRail(placed.reports.afterIndex.get(index));
  });
  pushRail(placed.reports.trailing);
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

function scrollToTop(rows: NormalizedChatMessage[], live: readonly LiveOutboundReport[] = FULL_STORE) {
  let state = snapshot(rows);
  let visible = WINDOW;
  const steps = [renderedColumn({ state, visible, live })];
  while ((transcriptBackfillCursor(state) ?? 0) > 0 || steps[steps.length - 1].length < rows.length) {
    if ((transcriptBackfillCursor(state) ?? 0) > 0) state = backfill(state, rows);
    visible += WINDOW;
    steps.push(renderedColumn({ state, visible, live }));
    if (steps.length > 40) throw new Error("paging never reached the start");
  }
  return { state, steps };
}

/** Every finished report call is followed directly by its own card, exactly once. */
function assertCardsFollowCalls(column: ReadonlyArray<{ key: string }>, turns: number, label: string) {
  assert.equal(column.filter(item => item.key.startsWith("report-")).length, turns, `${label}: one card per report`);
  for (let turn = 0; turn < turns; turn++) {
    const call = column.findIndex(item => item.key === `call-${turn}`);
    assert.equal(column[call + 1]?.key, `report-${turn}`, `${label}: report ${turn} follows its own tool call`);
  }
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
  for (const [label, live] of [["live store", FULL_STORE], ["empty store", []]] as const) {
    const { steps } = scrollToTop(allRows(TURNS), live);
    steps.forEach((column, index) => assertChronological(column, `${label} page ${index}`));
    for (let index = 1; index < steps.length; index++) assertPrependOnly(steps[index - 1], steps[index], `${label} page ${index}`);
    const final = steps[steps.length - 1];
    assert.equal(final.filter(item => item.key.startsWith("call-")).length, TURNS, `${label}: identical tool calls stay distinct rows`);
    assertCardsFollowCalls(final, TURNS, label);
  }
});

test("a reload in the middle rebuilds every card from the transcript with an empty live store", () => {
  const before = scrollToTop(allRows(TURNS - 2), liveStore(range(0, TURNS - 2))).steps.slice(-1)[0]!;
  const after = scrollToTop(allRows(TURNS), liveStore(range(TURNS - 2, TURNS))).steps.slice(-1)[0]!;
  assertChronological(after, "after reload");
  assertCardsFollowCalls(after, TURNS, "after reload");
  const reloadedPrefix = after.filter(item => before.some(old => old.key === item.key));
  assert.deepEqual(reloadedPrefix.map(item => item.key), before.map(item => item.key), "history reads the same after reload");
});

test("a live receipt that arrives before its call's result renders once, then belongs to that call", () => {
  const rows = [...allRows(TURNS - 1), ...turnRows(TURNS - 1, false)];
  const live = [liveReceipt(TURNS - 1)];
  const early = renderedColumn({ state: snapshot(rows), visible: WINDOW, live });
  assertChronological(early, "before the result");
  assert.equal(early.filter(item => item.key === `report-${TURNS - 1}`).length, 1, "the receipt shows once while its row is pending");
  const landed = renderedColumn({ state: snapshot(allRows(TURNS)), visible: WINDOW, live });
  assertChronological(landed, "after the result");
  const call = landed.findIndex(item => item.key === `call-${TURNS - 1}`);
  assert.equal(landed[call + 1]?.key, `report-${TURNS - 1}`, "the card follows its own call");
  assert.equal(landed.filter(item => item.key === `report-${TURNS - 1}`).length, 1, "the live receipt never duplicates the transcript card");
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
  assert.equal(view([send("queued", sentAt)], true).slice(-1)[0]?.key, "queued", "a fresh send waits after the running output");
  assert.equal(view([send("queued", sentAt, "written")], true).slice(-1)[0]?.key, "queued", "written input keeps waiting with its turn");
  assert.equal(view([send("queued", sentAt, "submit_unconfirmed")], false).slice(-1)[0]?.key, "queued",
    "at the end of its turn, queued input is about to be taken, so its echo may still land at the tail");
  assert.equal(view([send("queued", sentAt, "submit_unconfirmed")], true).slice(-1)[0]?.key, "queued");
  const unconfirmed = view([send("queued", sentAt, "submit_unconfirmed")], false);
  assert.deepEqual(neighbours(unconfirmed, "queued"), [`report-${TURNS - 1}`, `reply-${TURNS - 1}`],
    "once the following turn also ends without it, an unconfirmed send sits where it was sent");
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
  assert.equal(view([send("queued", sentAt, "held")]).slice(-1)[0]?.key, "queued");
  const failed = view([send("queued", sentAt, "failed")]);
  assert.deepEqual(neighbours(failed, "queued"), [`report-${TURNS - 1}`, `reply-${TURNS - 1}`]);
  assert.deepEqual(view([send("queued", sentAt, "failed")]), failed, "the failed bubble stays where it was sent");
});
