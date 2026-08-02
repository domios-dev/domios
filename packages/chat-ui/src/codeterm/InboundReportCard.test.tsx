import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { InboundReportCard, parseInboundReport } from "./InboundReportCard";
import { MessageBubble } from "../MessageBubble";
import { decodeCliMessageText, normalizeMessageText } from "./messageText";
import { COMPETITION_DISPATCH_READY_REPORT, COMPETITION_STARTUP_FAILED_REPORT, CURIE_PARTIAL_REPORT, CURIE_PARTIAL_REPORT_BODY_MARKERS, DISPATCH_READY_REPORT } from "./inboundReportFixtures.testdata";
import { mountInboundReportCardHarness } from "./InboundReportCard.browserHarness";
import { parseCoordinationReport } from "./coordinationReport";
import { inboundActivityEvent } from "./InboundActivityCard";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const content = "task: pl_d17yp2/task_2iw42e\nstatus: In Review\nbranch: lane/tool-cards\nsha: 1e096cba6\ntests: 11/11 green\nSTATUS: in_review | CHANGES: Added provider matrix";
const descriptor = parseInboundReport(content);
if (normalizeMessageText("one\r\ntwo\rthree") !== "one\ntwo\nthree") throw new Error("transport newline normalization regression");
if (decodeCliMessageText("one\\ntwo\\r\\nthree") !== "one\ntwo\nthree") throw new Error("CLI newline decoding regression");
const escapedDescriptor = parseInboundReport("status: in_review\\nchanges: first line\\ntests: green");
if (!escapedDescriptor.fields.some((field) => field.label === "changes" && field.value === "first line")) throw new Error("escaped report newline regression");
if (descriptor.task !== "pl_d17yp2/task_2iw42e") throw new Error("report task parse regression");
if (descriptor.status !== "in_review") throw new Error("latest report status must win");
if (!descriptor.fields.some((field) => field.label === "changes" && field.value === "Added provider matrix")) throw new Error("compound report fields parse regression");
if (descriptor.body !== "") throw new Error("a fully structured report must not invent a body");

// Real Curie payload: structured status plus a free-form multiline message.
const curie = parseInboundReport(CURIE_PARTIAL_REPORT);
if (curie.status !== "Partial") throw new Error(`report status regression: got ${JSON.stringify(curie.status)}`);
for (const marker of CURIE_PARTIAL_REPORT_BODY_MARKERS) {
  if (!curie.body.includes(marker)) throw new Error(`report body dropped line: ${JSON.stringify(marker)}`);
}
if (curie.body !== CURIE_PARTIAL_REPORT.split("\n").slice(1).join("\n").trim()) throw new Error("report body must survive verbatim below the structured block");

const curieHost = document.createElement("div");
document.body.append(curieHost);
const curieRoot = createRoot(curieHost);
await act(async () => curieRoot.render(
  <MessageBubble
    message={{ id: "report-curie", type: "user", content: CURIE_PARTIAL_REPORT, fromLabel: "Curie", fromTab: "02d1339c", inboundKind: "report" }}
  />,
));
for (const marker of CURIE_PARTIAL_REPORT_BODY_MARKERS) {
  if (curieHost.textContent?.includes(marker)) throw new Error(`collapsed partial report leaked body line: ${JSON.stringify(marker)}`);
}
if (!curieHost.textContent?.includes("Partial")) throw new Error("report card dropped the status chip");
const curieDetails = curieHost.querySelector('button[aria-label="Show report Details"]') as HTMLButtonElement | null;
if (!curieDetails) throw new Error("partial report Details disclosure missing");
await act(async () => curieDetails.click());
if (!curieHost.querySelector('[data-testid="chat-inbound-report-body"]')) throw new Error("expanded report card renders no body element");
for (const marker of CURIE_PARTIAL_REPORT_BODY_MARKERS) {
  if (!curieHost.textContent?.includes(marker)) throw new Error(`expanded report card dropped body line: ${JSON.stringify(marker)}`);
}
await act(async () => curieRoot.unmount());
curieHost.remove();

// The Playwright harness mounts the same fixture; keep its selectors honest here.
const harnessHost = document.createElement("div");
document.body.append(harnessHost);
let harnessRoot: ReturnType<typeof mountInboundReportCardHarness>;
await act(async () => { harnessRoot = mountInboundReportCardHarness(harnessHost); });
for (const variant of ["local", "mesh"]) {
  const card = harnessHost.querySelector(`[data-testid="report-variant-${variant}"] [data-testid="chat-inbound-report"]`);
  if (!card) throw new Error(`harness ${variant} report card missing`);
  const toggle = card.querySelector('button[aria-label="Show report Details"]') as HTMLButtonElement | null;
  if (!toggle) throw new Error(`harness ${variant} report Details missing`);
  await act(async () => toggle.click());
  const body = card.querySelector('[data-testid="chat-inbound-report-body"]');
  if (!body?.textContent?.includes("REPORT NOT SENT")) throw new Error(`harness ${variant} report body missing`);
  if (!card.textContent?.includes("Partial")) throw new Error(`harness ${variant} status chip missing`);
}
await act(async () => harnessRoot!.unmount());
harnessHost.remove();

const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
const focusCalls: number[] = [];
await act(async () => root.render(
  <InboundReportCard
    message={{ id: "report-1", type: "user", content }}
    sender="Hugo"
    expanded
    onToggle={() => {}}
    onFocusTab={() => focusCalls.push(1)}
  />,
));
if (!host.querySelector('[data-testid="chat-inbound-report"]')) throw new Error("typed report card missing");
if (!host.textContent?.includes("report · Hugo") || !host.textContent.includes("task_2iw42e") || !host.textContent.includes("1e096cba6")) throw new Error("report header regression");
if (host.querySelector('[data-testid="chat-cross-tab-bubble"]')) throw new Error("report must not contain a nested message bubble");
const focus = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Show Tab");
if (!focus) throw new Error("report source navigation missing");
await act(async () => (focus as HTMLButtonElement).click());
if (focusCalls.length !== 1) throw new Error("report source navigation regression");
await act(async () => root.unmount());
host.remove();

const dispatchHost = document.createElement("div");
document.body.append(dispatchHost);
const dispatchRoot = createRoot(dispatchHost);
await act(async () => dispatchRoot.render(
  <MessageBubble
    message={{ id: "report-2", type: "user", content, fromLabel: "Hugo", fromTab: "e7f14bec", inboundKind: "report" }}
  />,
));
if (!dispatchHost.querySelector('[data-testid="chat-inbound-report"]')) throw new Error("MessageBubble report dispatch regression");
if (dispatchHost.querySelector('[data-testid="chat-cross-tab-bubble"]')) throw new Error("MessageBubble routed report through generic bubble");
if (dispatchHost.textContent?.includes("Added provider matrix") || dispatchHost.textContent?.includes("1e096cba6")) throw new Error("in_review report leaked diagnostics while collapsed");
const reportDetails = dispatchHost.querySelector('button[aria-label="Show report Details"]') as HTMLButtonElement | null;
if (!reportDetails) throw new Error("in_review report Details disclosure missing");
await act(async () => reportDetails.click());
if (!dispatchHost.textContent?.includes("Added provider matrix") || !dispatchHost.textContent?.includes("1e096cba6")) throw new Error("in_review report lost expanded diagnostics");
await act(async () => dispatchRoot.unmount());
dispatchHost.remove();

async function renderCoordination(content: string, id: string) {
  const cardHost = document.createElement("div");
  document.body.append(cardHost);
  const cardRoot = createRoot(cardHost);
  await act(async () => cardRoot.render(
    <MessageBubble message={{ id, type: "user", content, fromLabel: "Kay", fromTab: "4fe93c04", inboundKind: "report" }} />,
  ));
  return { cardHost, cardRoot };
}

const coordination = await renderCoordination(DISPATCH_READY_REPORT, "coordination-ready");
const eventSummaryFixture = inboundActivityEvent({ id: "summary-type", type: "user", content: "" }, "pl_one/task_one", "success");
if (typeof eventSummaryFixture.summary !== "string" || eventSummaryFixture.summary !== "pl_one/task_one") throw new Error("inbound ToolEvent summary is not string-typed data");
const parsedCoordination = parseCoordinationReport(DISPATCH_READY_REPORT);
if (parsedCoordination?.diagnostics !== DISPATCH_READY_REPORT) throw new Error("parser changed untouched coordination diagnostics");
if (parseCoordinationReport(`Authored introduction\n${DISPATCH_READY_REPORT}`)) throw new Error("JSON-looking authored prose was misclassified as a coordination envelope");
if (parseCoordinationReport(`status: Done\n${DISPATCH_READY_REPORT.split("\n").slice(1).join("\n")}`)) throw new Error("authored report with a JSON body was misclassified as a coordination envelope");
if (parseCoordinationReport('task: pl_one/task_one\n{"kind":"worker_ack","plan_id":"pl_one","task_id":"task_one","state":"arbitrary"}')) throw new Error("unsupported worker_ack envelope was accepted");
if (parseCoordinationReport(COMPETITION_DISPATCH_READY_REPORT)?.task !== "pl_kddub3/task_compete") throw new Error("competition dispatch_ready without lane was rejected");
const competitionFailure = parseCoordinationReport(COMPETITION_STARTUP_FAILED_REPORT);
if (competitionFailure?.task !== "pl_kddub3/task_compete" || competitionFailure.state !== "Retry required" || competitionFailure.diagnostics !== COMPETITION_STARTUP_FAILED_REPORT) throw new Error("competition startup failure did not use exact injected task identity and raw diagnostics");
if (parseCoordinationReport(COMPETITION_DISPATCH_READY_REPORT.replace("task_compete", "different_task"))) throw new Error("mismatched header and payload identity was accepted");
const collapsed = coordination.cardHost.querySelector('[data-testid="chat-coordination-report"]');
if (!collapsed) throw new Error("dispatch_ready coordination card missing");
for (const visible of ["Dispatch ready", "Kay", "pl_kddub3/task_ii7zgs", "Awaiting worker acknowledgement"]) {
  if (!collapsed.textContent?.includes(visible)) throw new Error(`collapsed coordination summary missing ${JSON.stringify(visible)}`);
}
for (const hidden of ["attempt_ts4pkq", "019f6c2f", "codeterm-worktrees", "canonical_project_path", "{\"attempt_id\""]) {
  if (collapsed.textContent?.includes(hidden)) throw new Error(`collapsed coordination card leaked ${JSON.stringify(hidden)}`);
}
const detailsToggle = coordination.cardHost.querySelector('button[aria-label="Show coordination Details"]') as HTMLButtonElement | null;
if (!detailsToggle) throw new Error("coordination Details disclosure is not accessible");
if (detailsToggle.getAttribute("aria-expanded") !== "false") throw new Error("collapsed coordination disclosure state is missing");
const detailsRegionId = detailsToggle.getAttribute("aria-controls");
if (detailsRegionId !== "chat-details-coordination-ready") throw new Error("coordination Details target id is not stable");
const collapsedRegion = coordination.cardHost.querySelector(`#${detailsRegionId}`) as HTMLElement | null;
if (!collapsedRegion || !collapsedRegion.hidden) throw new Error("collapsed aria-controls does not resolve to a hidden Details region");
await act(async () => detailsToggle.click());
const details = coordination.cardHost.querySelector('[data-testid="chat-coordination-details"]');
const controlledRegion = coordination.cardHost.querySelector(`#${detailsRegionId}`);
if (!controlledRegion || controlledRegion !== collapsedRegion || collapsedRegion.hidden || detailsToggle.getAttribute("aria-expanded") !== "true" || !controlledRegion.contains(details)) throw new Error("expanded coordination disclosure relationship is broken");
if (controlledRegion.getAttribute("role") !== "region") throw new Error("coordination Details target lacks region semantics");
if (details?.textContent !== `DetailsCopy${DISPATCH_READY_REPORT}`) throw new Error("expanded coordination diagnostics are not exact");
if (details.querySelector("script")) throw new Error("coordination diagnostics executed markup instead of escaping it");
let copied = "";
Object.defineProperty(window.navigator, "clipboard", { configurable: true, value: { writeText: async (value: string) => { copied = value; } } });
const copyButton = details.querySelector('button[aria-label="Copy coordination diagnostics"]') as HTMLButtonElement | null;
if (!copyButton) throw new Error("coordination diagnostics copy action missing");
await act(async () => copyButton.click());
if (copied !== DISPATCH_READY_REPORT) throw new Error("coordination diagnostics copy lost fidelity");
await act(async () => coordination.cardRoot.unmount());
coordination.cardHost.remove();

for (const fixture of [COMPETITION_DISPATCH_READY_REPORT, COMPETITION_STARTUP_FAILED_REPORT]) {
  const rendered = await renderCoordination(fixture, fixture === COMPETITION_DISPATCH_READY_REPORT ? "competition-ready" : "competition-failed");
  const card = rendered.cardHost.querySelector('[data-testid="chat-coordination-report"]');
  if (!card?.textContent?.includes("pl_kddub3/task_compete")) throw new Error("competition coordination card lost header-derived task identity");
  if (card.textContent.includes("private\\lane") || card.textContent.includes("competition worker failed")) throw new Error("competition coordination card leaked failure diagnostics while collapsed");
  const toggle = card.querySelector('button[aria-controls]') as HTMLButtonElement | null;
  if (!toggle) throw new Error("competition coordination Details disclosure missing");
  await act(async () => toggle.click());
  const raw = rendered.cardHost.querySelector('[data-testid="chat-coordination-details"]')?.textContent ?? "";
  if (!raw.endsWith(fixture)) throw new Error("competition coordination Details lost exact raw diagnostics");
  await act(async () => rendered.cardRoot.unmount());
  rendered.cardHost.remove();
}

for (const fixture of [
  { id: "ack", json: { kind: "worker_ack", plan_id: "pl_one", task_id: "task_one", tab_id: "secret-tab", attempt_id: "secret-attempt", state: "in_progress" }, event: "Worker acknowledged", state: "In progress" },
  { id: "failed", json: { kind: "dispatch_startup_failed", plan_id: "pl_one", task_id: "task_one", tab_id: "secret-tab", stage: "session", error: "No session with C:\\secret\\lane" }, event: "Worker startup failed", state: "Retry required" },
]) {
  const rendered = await renderCoordination(`task: pl_one/task_one\n${JSON.stringify(fixture.json)}`, fixture.id);
  if (!rendered.cardHost.textContent?.includes(fixture.event) || !rendered.cardHost.textContent.includes(fixture.state)) throw new Error(`${fixture.id} summary regression`);
  if (rendered.cardHost.textContent.includes("secret-tab")) throw new Error(`${fixture.id} leaked internal ids while collapsed`);
  if (rendered.cardHost.textContent.includes("secret\\lane")) throw new Error(`${fixture.id} leaked arbitrary error text while collapsed`);
  await act(async () => rendered.cardRoot.unmount());
  rendered.cardHost.remove();
}

const proseReport = await renderCoordination("status: Done\nOrdinary Worker prose with pl_one/task_one stays readable.", "ordinary-prose");
if (proseReport.cardHost.querySelector('[data-testid="chat-coordination-report"]')) throw new Error("ordinary authored report misclassified as coordination JSON");
if (!proseReport.cardHost.textContent?.includes("Ordinary Worker prose with pl_one/task_one stays readable.")) throw new Error("ordinary authored report prose changed");
await act(async () => proseReport.cardRoot.unmount());
proseReport.cardHost.remove();

const sendMessage = { id: "send-1", type: "tool_call" as const, content: "", toolKind: "codeterm" as const, toolSummary: 'codeterm send --tab e3fa0754 "hello\\nHugo"', toolDetail: 'codeterm send --tab e3fa0754 "hello\\nHugo"' };
const sendHost = document.createElement("div");
document.body.append(sendHost);
const sendRoot = createRoot(sendHost);
await act(async () => sendRoot.render(
  <MessageBubble
    message={sendMessage}
    item={{ kind: "tool", event: { id: "send-1", phase: "call", status: "success", kind: "codeterm", summary: sendMessage.toolSummary, detail: sendMessage.toolDetail, message: sendMessage } }}
  />,
));
if (!sendHost.querySelector('[data-testid="codeterm-message-content"]') || !sendHost.textContent?.includes("hello\nHugo")) throw new Error("send card must decode line breaks and expand by default");
if (!sendHost.querySelector("p")?.className.includes("whitespace-pre-wrap")) throw new Error("message prose must preserve soft line breaks");
await act(async () => sendRoot.unmount());
sendHost.remove();

const localHost = document.createElement("div");
document.body.append(localHost);
const localRoot = createRoot(localHost);
await act(async () => localRoot.render(<MessageBubble message={{ id: "local-1", type: "user", content: "local one\\nlocal two\\n\\nlocal three" }} />));
if (!localHost.textContent?.includes("local one\nlocal two\n\nlocal three")) throw new Error("local user message newline decoding regression");
await act(async () => localRoot.unmount());
localHost.remove();
console.log("inbound report card tests passed");
