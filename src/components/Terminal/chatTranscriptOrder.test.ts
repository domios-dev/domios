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
import { mergeAccountNoticeRows } from "./accountNoticeRows";
import { reportVerdict } from "@codeterm/chat-ui";
import { __setListenForTest } from "../../lib/bridge";
import {
  hydrateOutboundAgentReportSnapshot,
  ingestAgentReport,
  outboundReportSnapshotRequest,
  readAgentReportProjection,
  resetAgentReportProjection,
  type AgentReportEventPayload,
} from "./agentReportProjection";

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
    at: turnAt(turn) + 12_000,
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
  hostRows?: readonly NormalizedChatMessage[];
  sends?: readonly DeliverySend[];
  turnRunning?: boolean;
  memory?: DeliveryProjectionMemory;
}

const memory = () => createDeliveryProjectionMemory("session");

/** The rendered column, top to bottom, through the same models TabSessionChat composes. */
function renderedColumn({ state, visible, live = FULL_STORE, hostRows = [], sends = [], turnRunning = false, memory: projectionMemory = memory() }: ViewInput) {
  const liveSends = deliveryProjectionSends(sends, null, queuedDeliveryIds(sends, turnRunning, projectionMemory));
  const projection = projectTranscript(state, liveSends);
  const transcript = mergeAccountNoticeRows(projection.rows, hostRows);
  const entries = stableDeliveryEntries(conversationEntries(transcript), liveSends, projection.echoes, projectionMemory);
  const start = Math.max(0, entries.length - visible);
  const window = entries.slice(start);
  const cards = outboundReportRows({ rows: window, transcript, live, recipient: (tab) => tab ?? "parent" });
  const placed = placeChatRails({
    visible: true,
    hiddenBefore: start > 0 || (transcriptBackfillCursor(state) ?? 0) > 0,
    hiddenAfter: false,
    messages: window,
    messageTimeMs: ({ msg }) => timestampMs(msg.timestamp),
    reportTimes: cards.rail.map(report => report.at),
    reportPositions: {
      sourceMessageIds: transcript.map(msg => msg.sourceMessageId ?? msg.id),
      messageId: ({ msg }) => msg.sourceMessageId ?? msg.id,
      reportMessageIds: cards.rail.map(report => report.position?.source_session_id === authority.sourceSessionId ? report.position.message_id : null),
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
function assertCardsAtReportTimes(column: ReadonlyArray<{ key: string; at: number }>, turns: number, label: string) {
  assert.equal(column.filter(item => item.key.startsWith("report-")).length, turns, `${label}: one card per report`);
  for (let turn = 0; turn < turns; turn++) {
    const report = column.findIndex(item => item.key === `report-${turn}`);
    const call = column.findIndex(item => item.key === `call-${turn}`);
    const reply = column.findIndex(item => item.key === `reply-${turn}`);
    assert.equal(column[report]?.at, turnAt(turn) + 12_000, `${label}: report ${turn} keeps its report receipt time`);
    assert.ok(report > call && report < reply, `${label}: report ${turn} sits between its call and later reply`);
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
    assertCardsAtReportTimes(final, TURNS, label);
  }
});

test("a reload in the middle rebuilds every card from the transcript with an empty live store", () => {
  const before = scrollToTop(allRows(TURNS - 2), liveStore(range(0, TURNS - 2))).steps.slice(-1)[0]!;
  const after = scrollToTop(allRows(TURNS), liveStore(range(TURNS - 2, TURNS))).steps.slice(-1)[0]!;
  assertChronological(after, "after reload");
  assertCardsAtReportTimes(after, TURNS, "after reload");
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
  const report = landed.findIndex(item => item.key === `report-${TURNS - 1}`);
  assert.equal(landed[report]?.at, turnAt(TURNS - 1) + 12_000, "the card keeps the result's report time");
  assert.ok(report > call, "the card renders at its timestamp after the issuing call");
  assert.equal(landed.filter(item => item.key === `report-${TURNS - 1}`).length, 1, "the live receipt never duplicates the transcript card");
});

test("history resync, a resumed VM epoch, a host row and a live report share one chronological column", () => {
  const rows = turnRows(0).map((row, index) => ({
    ...row,
    sourceSessionId: index < 2 ? "vm-before-resume" : "vm-after-resume",
    seq: index < 2 ? index : index - 2,
  })).reverse();
  const hostRows: NormalizedChatMessage[] = [{ id: "host-status", type: "system", content: "opaque host event", timestamp: iso(turnAt(0) + 15_000) }];
  const column = renderedColumn({
    state: snapshot(rows),
    visible: 20,
    live: [{ ...liveReceipt(0), at: turnAt(0) + 11_000 }],
    hostRows,
  });
  assertChronological(column, "resynchronized resumed transcript");
  assert.deepEqual(column.map(item => item.key), ["user-0", "call-0", "report-0", "host-status", "reply-0"]);
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

test("timestamped startup status stays anchored above later untimed ACP rows, tool patches, and reentry", () => {
  const startAt = START;
  const prompt: NormalizedChatMessage = { id: "prompt-0", type: "user", content: "assigned task", timestamp: iso(startAt) };
  const ackRow: NormalizedChatMessage = { id: "ack-0", type: "assistant", content: "acknowledging" };
  const workRow: NormalizedChatMessage = { id: "work-1", type: "assistant", content: "working" };
  const toolRow: NormalizedChatMessage = { id: "tool-1", type: "tool_call", content: "running", toolName: "Bash", toolKind: "command", toolId: "t-1" };
  const readyCard: LiveOutboundReport = {
    targetTabId: "elgar001",
    at: startAt + 5_000,
    position: { source_session_id: authority.sourceSessionId, message_id: "ack-0" },
    descriptor: {
      reportId: "rep-ready",
      recipient: "Elgar",
      task: "pl_7lsisa/task_uw3aw9",
      status: null,
      sha: null,
      message: "opaque startup notice",
      taskOutcome: null,
    },
  };
  const foreignCard: LiveOutboundReport = {
    ...readyCard,
    at: startAt + 6_000,
    position: { source_session_id: "other-session", message_id: "ack-0" },
    descriptor: { ...readyCard.descriptor, reportId: "rep-foreign" },
  };
  const finalReport: LiveOutboundReport = {
    targetTabId: "elgar001",
    at: startAt + 14 * 60_000,
    position: { source_session_id: authority.sourceSessionId, message_id: "tool-1" },
    descriptor: {
      reportId: "rep-final",
      recipient: "Elgar",
      task: "pl_7lsisa/task_uw3aw9",
      status: "in_review",
      sha: "4f5d23dff5",
      message: "opaque final report",
      taskOutcome: { persisted: true, transitionRejected: false, cause: null, reportOutcome: null },
    },
  };

  assert.equal(reportVerdict(readyCard.descriptor.status, readyCard.descriptor.taskOutcome), "sent");
  assert.equal(reportVerdict(finalReport.descriptor.status, finalReport.descriptor.taskOutcome), "in_review");

  let state = reduceTranscript(createTranscriptState(), {
    kind: "snapshot", authority, range: null, rows: [prompt, ackRow, workRow, toolRow],
  }).state;
  const liveBeforePatch = renderedColumn({ state, visible: 20, live: [readyCard, readyCard] });
  assert.deepEqual(liveBeforePatch.map(item => item.key), ["prompt-0", "ack-0", "report-ready", "work-1", "tool-1"]);

  state = reduceTranscript(state, {
    kind: "delta", authority, range: null, rows: [{ ...toolRow, content: "completed" }],
  }).state;
  const liveAfterPatch = renderedColumn({ state, visible: 20, live: [readyCard, finalReport] });
  assert.deepEqual(liveAfterPatch.map(item => item.key), ["prompt-0", "ack-0", "report-ready", "work-1", "tool-1", "report-final"]);

  const reentry = renderedColumn({
    state: reduceTranscript(createTranscriptState(), {
      kind: "snapshot", authority, range: null, rows: [prompt, ackRow, workRow, { ...toolRow, content: "completed" }],
    }).state,
    visible: 20,
    live: [readyCard, finalReport],
  });
  assert.deepEqual(reentry.map(item => item.key), liveAfterPatch.map(item => item.key));

  const withForeign = renderedColumn({ state, visible: 20, live: [readyCard, foreignCard] });
  assert.deepEqual(withForeign.map(item => item.key), ["prompt-0", "ack-0", "report-ready", "work-1", "tool-1", "report-foreign"]);
});

test("actual emitted startup agent-report envelope projects one neutral anchored card across tool patches and reentry", () => {
  resetAgentReportProjection();
  try {
    const startAt = START;
    const prompt: NormalizedChatMessage = { id: "prompt-0", type: "user", content: "assigned task", timestamp: iso(startAt) };
    const ackRow: NormalizedChatMessage = { id: "ack-0", type: "assistant", content: "acknowledging" };
    const workRow: NormalizedChatMessage = { id: "work-1", type: "assistant", content: "working" };
    const toolRow: NormalizedChatMessage = { id: "tool-1", type: "tool_call", content: "running", toolName: "Bash", toolKind: "command", toolId: "t-1" };
    const emitted: AgentReportEventPayload = {
      target_tab_id: "elgar001",
      from_tab_id: "order001",
      from_title: "Order",
      message: "Agent Order (acp) is ready for task task_uw3aw9, attempt attempt_l33cqd.",
      kind: "dispatch_ready",
      sender_position: { source_session_id: authority.sourceSessionId, message_id: "ack-0" },
      report: {
        id: "rep-emitted-ready",
        from_tab_id: "order001",
        from_title: "Order",
        message: "Agent Order (acp) is ready for task task_uw3aw9, attempt attempt_l33cqd.",
        timestamp: Math.floor((startAt + 5_000) / 1000),
        plan_id: "pl_7lsisa",
        task_id: "task_uw3aw9",
      },
    };
    assert.equal(ingestAgentReport(emitted), true);
    assert.equal(ingestAgentReport(emitted), false, "duplicate startup report id is ignored");
    const projectedLive = (): LiveOutboundReport[] => readAgentReportProjection("order001").outbound.map((report) => ({
      targetTabId: report.targetTabId,
      at: report.timestamp,
      position: report.senderPosition,
      descriptor: {
        reportId: report.reportId,
        recipient: "Elgar",
        task: report.task,
        status: report.status,
        sha: report.sha,
        message: report.message,
        ...(report.kind ? { kind: report.kind } : {}),
        taskOutcome: null,
      },
    }));
    const live = projectedLive();
    assert.equal(live.length, 1);
    assert.equal(live[0].descriptor.kind, "dispatch_ready");
    assert.equal(live[0].descriptor.status, null);
    assert.equal(reportVerdict(live[0].descriptor.status, live[0].descriptor.taskOutcome), "sent");

    let state = reduceTranscript(createTranscriptState(), {
      kind: "snapshot", authority, range: null, rows: [prompt, ackRow, workRow, toolRow],
    }).state;
    const beforePatch = renderedColumn({ state, visible: 20, live });
    assert.deepEqual(beforePatch.map(item => item.key), ["prompt-0", "ack-0", "report-emitted-ready", "work-1", "tool-1"]);

    state = reduceTranscript(state, {
      kind: "delta", authority, range: null, rows: [{ ...toolRow, content: "patched" }],
    }).state;
    const afterPatch = renderedColumn({ state, visible: 20, live: projectedLive() });
    assert.deepEqual(afterPatch.map(item => item.key), ["prompt-0", "ack-0", "report-emitted-ready", "work-1", "tool-1"]);

    const reentry = renderedColumn({
      state: reduceTranscript(createTranscriptState(), {
        kind: "snapshot", authority, range: null, rows: [prompt, ackRow, workRow, { ...toolRow, content: "patched" }],
      }).state,
      visible: 20,
      live: projectedLive(),
    });
    assert.deepEqual(reentry.map(item => item.key), afterPatch.map(item => item.key));
  } finally {
    resetAgentReportProjection();
  }
});

test("cold host snapshot restores the original startup card once before later untimed rows", async () => {
  resetAgentReportProjection();
  let subscribed = false;
  let receive: ((event: { payload: AgentReportEventPayload }) => void) | null = null;
  __setListenForTest(<T>(event: string, handler: (value: { payload: T }) => void) => {
    assert.equal(event, "agent-report");
    receive = handler as (value: { payload: AgentReportEventPayload }) => void;
    subscribed = true;
    return Promise.resolve(() => { subscribed = false; });
  });
  try {
    const request = outboundReportSnapshotRequest("order001", {
      peerId: null, sessionId: "session", bindingNonce: "launch", bootGeneration: 2, runtimeEpoch: 7, requestContext: 3,
    });
    assert.ok(request);
    assert.equal(request.path, "/api/tabs/order001/reports");
    assert.equal(request.headers["X-CodeTerm-Session-Id"], "session");
    assert.equal(request.headers["X-CodeTerm-Binding-Nonce"], "launch");
    assert.equal(outboundReportSnapshotRequest("mesh:peer/order001", {
      peerId: "peer", sessionId: "session", bindingNonce: "launch", bootGeneration: 2, runtimeEpoch: 7, requestContext: 3,
    }), null, "remote tabs cannot read a local sender index");
    assert.equal(outboundReportSnapshotRequest("order001", {
      peerId: null, sessionId: "session", bindingNonce: null, bootGeneration: 2, runtimeEpoch: 7, requestContext: 3,
    }), null, "a missing binding nonce cannot authorize a read");

    const startAt = START;
    const prompt: NormalizedChatMessage = { id: "prompt-0", type: "user", content: "assigned task", timestamp: iso(startAt) };
    const ackRow: NormalizedChatMessage = { id: "ack-0", type: "assistant", content: "acknowledging" };
    const workRow: NormalizedChatMessage = { id: "work-1", type: "assistant", content: "working" };
    const toolRow: NormalizedChatMessage = { id: "tool-1", type: "tool_call", content: "patched", toolName: "Bash", toolKind: "command", toolId: "tool-1" };
    const event: AgentReportEventPayload = {
      target_tab_id: "elgar001", from_tab_id: "order001", from_title: "Order", message: "opaque ready",
      report: { id: "rep-cold-ready", from_tab_id: "order001", from_title: "Order", message: "opaque ready",
        timestamp: Math.floor((startAt + 5_000) / 1000), status: null },
    };
    const snapshot = [{
      target_tab_id: "elgar001",
      report: {
        ...event.report,
        kind: "dispatch_ready",
        sender_position: { source_session_id: "session", message_id: "ack-0" },
      },
    }];
    const hydrated = await hydrateOutboundAgentReportSnapshot("order001", request.scopeKey, async () => {
      assert.equal(subscribed, true, "the event listener is installed before the snapshot read");
      assert.ok(receive);
      receive!({ payload: event });
      return snapshot;
    }, new AbortController().signal);
    assert.equal(hydrated, true);
    const report = readAgentReportProjection("order001").outbound[0];
    assert.ok(report);
    assert.equal(report.reportId, "rep-cold-ready");
    assert.equal(report.timestamp, startAt + 5_000);
    assert.equal(report.status, null);
    assert.equal(report.kind, "dispatch_ready");
    assert.deepEqual(report.senderPosition, { source_session_id: "session", message_id: "ack-0" });
    assert.equal(readAgentReportProjection("order001").outbound.length, 1, "event/read overlap merges by the original id");

    const live: LiveOutboundReport[] = [report].map((row) => ({
      targetTabId: row.targetTabId,
      at: row.timestamp,
      position: row.senderPosition,
      descriptor: {
        reportId: row.reportId, recipient: "Elgar", task: row.task, status: row.status, sha: row.sha,
        message: row.message, kind: row.kind,
        taskOutcome: row.taskOutcome && {
          persisted: row.taskOutcome.persisted,
          transitionRejected: row.taskOutcome.transition_rejected,
          cause: row.taskOutcome.cause ?? null,
          reportOutcome: row.taskOutcome.report_outcome ?? null,
        },
      },
    }));
    assert.equal(reportVerdict(live[0]!.descriptor.status, live[0]!.descriptor.taskOutcome), "sent");
    const state = reduceTranscript(createTranscriptState(), {
      kind: "snapshot", authority, range: null, rows: [prompt, ackRow, workRow, toolRow],
    }).state;
    const column = renderedColumn({ state, visible: 20, live });
    assert.deepEqual(column.map((item) => item.key), ["prompt-0", "ack-0", "report-cold-ready", "work-1", "tool-1"]);

    resetAgentReportProjection();
    const coldReloaded = await hydrateOutboundAgentReportSnapshot("order001", request.scopeKey, async () => snapshot,
      new AbortController().signal);
    assert.equal(coldReloaded, true);
    const coldReport = readAgentReportProjection("order001").outbound[0];
    assert.ok(coldReport);
    assert.deepEqual({
      id: coldReport.reportId,
      timestamp: coldReport.timestamp,
      status: coldReport.status,
      position: coldReport.senderPosition,
      kind: coldReport.kind,
    }, {
      id: report.reportId,
      timestamp: report.timestamp,
      status: report.status,
      position: report.senderPosition,
      kind: report.kind,
    }, "a cold projection reload keeps the original report metadata");
    const coldColumn = renderedColumn({
      state,
      visible: 20,
      live: [{ ...live[0]!, at: coldReport.timestamp, position: coldReport.senderPosition }],
    });
    assert.deepEqual(coldColumn.map((item) => item.key), column.map((item) => item.key));
  } finally {
    resetAgentReportProjection();
    __setListenForTest(null);
  }
});

test("snapshot generation fences late reads after session, nonce, peer, boot, epoch, or relay context changes", async () => {
  __setListenForTest(async () => () => {});
  const base = { peerId: null, sessionId: "session", bindingNonce: "nonce", bootGeneration: 8, runtimeEpoch: 2, requestContext: 4 };
  const original = outboundReportSnapshotRequest("order001", base)!;
  for (const authority of [
    { ...base, sessionId: "replacement-session" },
    { ...base, bindingNonce: "replacement-nonce" },
    { ...base, peerId: "peer" },
    { ...base, bootGeneration: 9 },
    { ...base, runtimeEpoch: 3 },
    { ...base, requestContext: 5 },
  ]) {
    const changed = outboundReportSnapshotRequest("order001", authority);
    assert.ok(!changed || changed.scopeKey !== original.scopeKey, "every authority generation has a distinct read scope");
  }

  resetAgentReportProjection();
  let resolveOld: ((snapshot: unknown) => void) | null = null;
  let started!: () => void;
  const readStarted = new Promise<void>((resolve) => { started = resolve; });
  const controller = new AbortController();
  const oldRead = hydrateOutboundAgentReportSnapshot("order001", original.scopeKey, () => {
    started();
    return new Promise((resolve) => { resolveOld = resolve; });
  }, controller.signal);
  await readStarted;
  const next = outboundReportSnapshotRequest("order001", { ...base, bindingNonce: "replacement-nonce" })!;
  assert.equal(await hydrateOutboundAgentReportSnapshot("order001", next.scopeKey, async () => [], new AbortController().signal), true,
    "an empty supported snapshot is a successful empty result");
  resolveOld!([{
    target_tab_id: "foreign-target",
    report: { id: "rep-stale", from_tab_id: "order001", timestamp: Math.floor(START / 1000) },
  }]);
  assert.equal(await oldRead, false, "late snapshots from a replaced binding are ignored");
  assert.equal(readAgentReportProjection("order001").outbound.length, 0);
  await assert.rejects(hydrateOutboundAgentReportSnapshot("order001", next.scopeKey, async () => {
    throw new Error("read refused");
  }, new AbortController().signal), /read refused/);
  assert.equal(readAgentReportProjection("order001").outbound.length, 0, "refused reads do not fabricate cards");
  resetAgentReportProjection();
  __setListenForTest(null);
});
