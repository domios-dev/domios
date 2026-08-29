// Run: npx tsx packages/chat-ui/src/codeterm/reportCard.test.tsx
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { InboundReportCard, parseInboundReport } from "./InboundReportCard";
import { OutboundReportCard, type OutboundReportDescriptor } from "./OutboundReportCard";
import { parseDeliveredTaskOutcome, reportContentBody, reportVerdict, shortSha } from "./reportOutcome";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const REPORT_ID = "79c177b3-0d8d-43f3-a16f-7e9b9412a294";
const SHA = "f9606ded342df352987af64d4b47c02393b79c10";
const SUMMARY = "Both transcript REDs are closed and the lane tip is ready for integration.";

/** Exactly what the daemon delivers for a report whose task transition was
 *  refused: structured header, incident block, then the report itself. */
const DELIVERED_REJECTED = [
  "task: pl_chatui/task_transcript",
  "status: In Review",
  "branch: lane/chatui-transcript-authority-opus",
  `sha: ${SHA}`,
  "REPORT RECEIVED — TASK TRANSITION REJECTED",
  "The report itself was delivered and must not be resent.",
  "Owner action required: missing caller session id",
  "",
  SUMMARY,
].join("\n");

async function mount(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, root };
}

async function main(): Promise<void> {
  // The delivered incident header is structured truth, not prose matching.
  const outcome = parseDeliveredTaskOutcome(parseInboundReport(DELIVERED_REJECTED).body);
  assert(outcome?.transitionRejected === true, "delivered rejection header was not recognised");
  assert(outcome?.persisted === false, "a refused transition was parsed as persisted");
  assert(outcome?.cause === "missing caller session id", `rejection cause was ${outcome?.cause}`);
  assert(
    parseDeliveredTaskOutcome(`${SUMMARY}\nREPORT RECEIVED — TASK TRANSITION REJECTED`) === null,
    "quoted incident prose was misread as an incident header",
  );
  assert(
    reportContentBody(parseInboundReport(DELIVERED_REJECTED).body) === SUMMARY,
    "the report summary did not survive the incident header",
  );
  assert(shortSha(SHA) === "f9606ded3", `short sha was ${shortSha(SHA)}`);
  assert(reportVerdict("Done", null) === "Done", "a plain status lost its verdict");
  assert(
    reportVerdict("Done", { persisted: false, transitionRejected: true, cause: null })
      === "transition rejected",
    "a refused transition did not win the header verdict",
  );

  // Inbound: one dense header row, then the summary — no repeated metadata rows.
  const inbound = await mount(
    <InboundReportCard
      message={{ id: "inbound-report", type: "user", content: DELIVERED_REJECTED }}
      sender="ChatUI fixer"
      expanded
      onToggle={() => {}}
      onFocusTab={() => {}}
    />,
  );
  const card = inbound.host.querySelector('[data-testid="chat-inbound-report"]');
  assert(card, "inbound report card missing");
  assert(inbound.host.textContent?.includes("Report from ChatUI fixer"), "inbound header lost its sender");
  assert(inbound.host.textContent?.includes("task_transcript"), "inbound header lost its task");
  assert(inbound.host.textContent?.includes("transition rejected"), "inbound header lost its verdict");
  assert(inbound.host.textContent?.includes("f9606ded3"), "inbound header lost its short sha");
  assert(!inbound.host.textContent?.includes(SHA), "inbound header rendered the full sha");
  const definitions = [...inbound.host.querySelectorAll("dt")].map((node) => node.textContent);
  for (const repeated of ["task", "status", "branch", "sha"]) {
    assert(!definitions.includes(repeated), `expanded body repeated the header field "${repeated}"`);
  }
  const incidentRow = inbound.host.querySelector('[data-testid="chat-inbound-report-incident"]');
  assert(
    incidentRow?.textContent?.includes("missing caller session id"),
    "the inbound card dropped the rejection cause",
  );
  const inboundBody = inbound.host.querySelector('[data-testid="chat-inbound-report-body"]');
  assert(inboundBody?.textContent === SUMMARY, "the inbound card did not lead with the report summary");
  assert(
    inboundBody?.parentElement?.className.includes("ct-compact-scroll")
      && inboundBody.parentElement.className.includes("overflow-y-auto")
      && inboundBody.parentElement.className.includes("max-h-64"),
    "a long inbound report is not internally scrollable",
  );
  const showTab = [...inbound.host.querySelectorAll("button")].find((node) => node.textContent === "Show Tab");
  assert(showTab, "inbound Show Tab action is not reachable");
  assert(
    inbound.host.querySelector('button[aria-label="Hide report Details"]'),
    "inbound disclosure control is not accessible",
  );
  await act(async () => inbound.root.unmount());
  inbound.host.remove();

  // Outbound: the same report, as its sender sees it, from structured truth
  // only — no transcript text, no delivery state, no screen scraping.
  const descriptor: OutboundReportDescriptor = {
    reportId: REPORT_ID,
    recipient: "Orchestrator",
    task: "pl_chatui/task_transcript",
    status: "In Review",
    sha: SHA,
    message: SUMMARY,
    taskOutcome: { persisted: false, transitionRejected: true, cause: "missing caller session id" },
  };
  const outbound = await mount(
    <OutboundReportCard report={descriptor} onFocusTab={() => {}} defaultExpanded />,
  );
  assert(
    outbound.host.querySelectorAll('[data-testid="chat-outbound-report"]').length === 1,
    "the sender rendered other than exactly one outbound report card",
  );
  assert(outbound.host.textContent?.includes("Report to Orchestrator"), "outbound header lost its recipient");
  assert(outbound.host.textContent?.includes("pl_chatui/task_transcript"), "outbound header lost its task");
  assert(outbound.host.textContent?.includes("transition rejected"), "outbound header lost its verdict");
  assert(outbound.host.textContent?.includes("f9606ded3"), "outbound header lost its short sha");
  assert(
    outbound.host.querySelector('[data-testid="chat-outbound-report-incident"]')?.textContent
      ?.includes("missing caller session id"),
    "the outbound card dropped the rejection cause",
  );
  assert(
    outbound.host.querySelector('[data-testid="chat-outbound-report-body"]')?.textContent === SUMMARY,
    "the outbound card did not lead with the report summary",
  );
  assert(
    outbound.host.querySelector('[data-testid="chat-outbound-report-body"]')?.parentElement
      ?.className.includes("ct-compact-scroll"),
    "the outbound report does not use the compact scroll surface",
  );
  assert(
    [...outbound.host.querySelectorAll("button")].some((node) => node.textContent === "Show Tab"),
    "outbound Show Tab action is not reachable",
  );
  const outboundDisclosure = outbound.host.querySelector('button[aria-label="Hide report Details"]') as HTMLButtonElement | null;
  assert(outboundDisclosure, "outbound disclosure control is not accessible");
  await act(async () => outboundDisclosure.click());
  assert(
    outbound.host.querySelector('button[aria-label="Show report Details"]'),
    "outbound disclosure did not collapse",
  );
  assert(
    outbound.host.querySelector('[data-testid="chat-outbound-report"]'),
    "a collapsed outbound report card disappeared",
  );
  assert(
    outbound.host.textContent?.includes("transition rejected"),
    "a collapsed outbound card hid the rejection verdict",
  );
  await act(async () => outbound.root.unmount());
  outbound.host.remove();

  // A persisted report keeps its own status as the verdict on both sides.
  const persisted = await mount(
    <OutboundReportCard
      report={{
        ...descriptor,
        reportId: "5f1c0f1e-1f2a-4c1a-9f31-2b7b6f2f0c11",
        status: "Done",
        taskOutcome: { persisted: true, transitionRejected: false, cause: null },
      }}
      defaultExpanded
    />,
  );
  assert(persisted.host.textContent?.includes("Done"), "a persisted report lost its status verdict");
  assert(
    !persisted.host.querySelector('[data-testid="chat-outbound-report-incident"]'),
    "a persisted report invented an incident row",
  );
  await act(async () => persisted.root.unmount());
  persisted.host.remove();

  console.log("PASS report cards: one dense header row, summary first, rejection truth on both sides");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
