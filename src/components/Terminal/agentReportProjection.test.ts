// Run: npx tsx src/components/Terminal/agentReportProjection.test.ts
import { __setListenForTest } from "../../lib/bridge";
import {
  AGENT_REPORT_EVENT,
  ingestAgentReport,
  readAgentReportProjection,
  resetAgentReportProjection,
  subscribeAgentReportProjection,
  type AgentReportEventPayload,
} from "./agentReportProjection";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const WORKER_TAB = "166df18d";
const ORCHESTRATOR_TAB = "e7f14bec";

/** The exact delivered-but-refused submission: the parent received it, the task
 *  transition was refused for a missing caller session id, nothing persisted. */
const REJECTED_REPORT: AgentReportEventPayload = {
  target_tab_id: ORCHESTRATOR_TAB,
  from_tab_id: WORKER_TAB,
  from_title: "ChatUI fixer",
  message: "REPORT RECEIVED — TASK TRANSITION REJECTED\nlane merged, transcript REDs closed",
  report: {
    id: "79c177b3-0d8d-43f3-a16f-7e9b9412a294",
    plan_id: "pl_chatui",
    task_id: "task_transcript",
    status: "in_review",
    branch: "lane/chatui-transcript-authority-opus",
    sha: "f9606ded342df352987af64d4b47c02393b79c10",
    timestamp: 1_770_000_000,
  },
  task_report: {
    persisted: false,
    transition_rejected: true,
    plan_id: "pl_chatui",
    task_id: "task_transcript",
    cause: "missing caller session id",
  },
};

const ACCEPTED_REPORT: AgentReportEventPayload = {
  target_tab_id: ORCHESTRATOR_TAB,
  from_tab_id: WORKER_TAB,
  from_title: "ChatUI fixer",
  message: "second lane green",
  report: {
    id: "5f1c0f1e-1f2a-4c1a-9f31-2b7b6f2f0c11",
    plan_id: "pl_chatui",
    task_id: "task_cards",
    status: "done",
    sha: "154fd0baf9c3",
    timestamp: 1_770_000_100,
  },
  task_report: {
    persisted: true,
    transition_rejected: false,
    plan_id: "pl_chatui",
    task_id: "task_cards",
    cause: null,
  },
};

/** A freeform report that resolved no plan task at all. */
const UNTASKED_REPORT: AgentReportEventPayload = {
  target_tab_id: ORCHESTRATOR_TAB,
  from_tab_id: WORKER_TAB,
  from_title: "ChatUI fixer",
  message: "no task context, still a report",
  report: { id: "0c1c9d54-6b7e-4a5c-a2a0-9d0e1b2c3d4e" },
  task_report: null,
};

async function main(): Promise<void> {
  const listened: string[] = [];
  let subscribed = false;
  let deliver: (event: { payload: AgentReportEventPayload }) => void = () => {
    throw new Error("the projection never subscribed to the report event");
  };
  __setListenForTest(async (event, handler) => {
    listened.push(event);
    deliver = handler as typeof deliver;
    subscribed = true;
    return () => {};
  });
  resetAgentReportProjection();

  let notifications = 0;
  const mounted = subscribeAgentReportProjection(WORKER_TAB, () => { notifications += 1; });
  await Promise.resolve();
  await Promise.resolve();
  assert(subscribed, "the projection never subscribed to the report event");

  assert(
    readAgentReportProjection(WORKER_TAB).outbound.length === 0
      && readAgentReportProjection(ORCHESTRATOR_TAB).inbound.length === 0,
    "an untouched tab projected a report",
  );

  assert(ingestAgentReport(REJECTED_REPORT), "an accepted report was not projected");

  // (i) the sender sees exactly one truthful outbound card.
  const sender = readAgentReportProjection(WORKER_TAB);
  assert(sender.outbound.length === 1, `sender projected ${sender.outbound.length} outbound cards`);
  assert(sender.inbound.length === 0, "sender projected its own report as inbound");
  const outbound = sender.outbound[0];
  assert(outbound.reportId === REJECTED_REPORT.report.id, "sender card lost its report identity");
  assert(outbound.counterpartTabId === ORCHESTRATOR_TAB, "sender card lost its target tab");
  assert(outbound.task === "pl_chatui/task_transcript", `sender card task ${outbound.task}`);
  assert(outbound.sha === REJECTED_REPORT.report.sha, "sender card lost the reported sha");

  // (ii) the orchestrator sees exactly one inbound card for the same report.
  const orchestrator = readAgentReportProjection(ORCHESTRATOR_TAB);
  assert(
    orchestrator.inbound.length === 1 && orchestrator.outbound.length === 0,
    "orchestrator did not project exactly one inbound card",
  );
  assert(
    orchestrator.inbound[0].reportId === REJECTED_REPORT.report.id,
    "inbound card is not the same report",
  );
  assert(
    orchestrator.inbound[0].counterpartTabId === WORKER_TAB,
    "inbound card lost its sender tab",
  );

  // (iii) the rejection truth survives on both sides, and a replayed event —
  // the inbound text marker's fallback path — never doubles the card.
  for (const side of [outbound, orchestrator.inbound[0]]) {
    assert(side.taskOutcome?.transition_rejected === true, "a side dropped the rejection truth");
    assert(side.taskOutcome?.persisted === false, "a refused transition was shown as persisted");
    assert(
      side.taskOutcome?.cause === "missing caller session id",
      "a side dropped the rejection cause",
    );
  }
  assert(!ingestAgentReport(REJECTED_REPORT), "a replayed report_id was projected twice");
  assert(
    !ingestAgentReport({
      ...REJECTED_REPORT,
      message: "the same report arriving through the text marker path",
    }),
    "a differently worded replay of one report_id was projected twice",
  );
  assert(
    readAgentReportProjection(WORKER_TAB).outbound.length === 1
      && readAgentReportProjection(ORCHESTRATOR_TAB).inbound.length === 1,
    "a replayed report duplicated a card",
  );

  // (iv) visibility never depends on transition success or on a task existing.
  assert(ingestAgentReport(ACCEPTED_REPORT), "a persisted report was not projected");
  assert(ingestAgentReport(UNTASKED_REPORT), "a report without a task was not projected");
  const allOutbound = readAgentReportProjection(WORKER_TAB).outbound;
  assert(allOutbound.length === 3, `sender projected ${allOutbound.length} of 3 reports`);
  assert(
    allOutbound.filter((card) => card.taskOutcome?.persisted === true).length === 1,
    "persisted truth is not per-report",
  );
  assert(
    allOutbound.some((card) => card.taskOutcome === null && card.task === null),
    "a report with no task outcome was hidden",
  );
  assert(
    readAgentReportProjection(ORCHESTRATOR_TAB).inbound.length === 3,
    "the orchestrator hid a report",
  );

  // (v) the ingest boundary owns the timestamp unit: daemon epoch-seconds
  // normalize to milliseconds exactly once, milliseconds pass through, and a
  const stamped = readAgentReportProjection(WORKER_TAB).outbound;
  assert(
    stamped.find((card) => card.reportId === REJECTED_REPORT.report.id)?.timestamp
      === 1_770_000_000_000,
    "an epoch-seconds report timestamp was not normalized to ms at ingest",
  );
  assert(
    stamped.find((card) => card.reportId === UNTASKED_REPORT.report.id)?.timestamp === null,
    "a report without a timestamp did not stay unpositioned",
  );
  const msStampedId = "3d9a6f0b-8e21-4f6a-b1d5-7c4e9a2f5b60";
  assert(
    ingestAgentReport({
      ...UNTASKED_REPORT,
      report: { id: msStampedId, timestamp: 1_770_000_200_000 },
    }),
    "a millisecond-stamped report was not projected",
  );
  assert(
    readAgentReportProjection(WORKER_TAB).outbound
      .find((card) => card.reportId === msStampedId)?.timestamp === 1_770_000_200_000,
    "an already-milliseconds timestamp did not pass through unchanged",
  );

  // Remount safety: the store outlives every subscriber, so a Chat that
  // unmounts and mounts again re-reads the same single card per report_id.
  const outboundBeforeRemount = readAgentReportProjection(WORKER_TAB).outbound.length;
  mounted();
  const remounted = subscribeAgentReportProjection(WORKER_TAB, () => { notifications += 1; });
  await Promise.resolve();
  assert(
    readAgentReportProjection(WORKER_TAB).outbound.length === outboundBeforeRemount,
    "a remount lost or duplicated projected reports",
  );
  assert(
    listened.filter((event) => event === AGENT_REPORT_EVENT).length === 1,
    `the projection owns ${listened.length} event listeners after a remount`,
  );

  // The live bridge path feeds the same ingest, so no card depends on screen
  // text, PTY state or delivery outcome.
  const liveId = "b71f2b28-6f4a-4b0e-9a3e-51fa2d7c8a90";
  deliver({ payload: { ...ACCEPTED_REPORT, report: { ...ACCEPTED_REPORT.report, id: liveId } } });
  assert(
    readAgentReportProjection(WORKER_TAB).outbound.some((card) => card.reportId === liveId),
    "a report delivered through the event bridge was not projected",
  );
  assert(notifications > 0, "a live report did not notify the mounted subscriber");
  remounted();

  resetAgentReportProjection();
  assert(
    readAgentReportProjection(WORKER_TAB).outbound.length === 0,
    "reset left projected reports behind",
  );
  console.log("PASS agent report projection: one card per report_id per side, both sides truthful");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
