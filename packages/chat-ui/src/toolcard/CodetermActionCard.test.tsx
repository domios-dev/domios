import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ToolEvent } from "@codeterm/chat-core";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { CodetermActionCard } from "./CodetermActionCard";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) Object.defineProperty(globalThis, name, { configurable: true, value });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PLAN = "pl_d17yp2";
const TASK = "task_4orgnd";

function event(command: string): ToolEvent {
  return { id: "id", phase: "result", status: "success", kind: "codeterm", summary: command, detail: command, message: { id: "id", type: "tool_call", content: "" } };
}

async function render(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, root };
}

function buttonByRoleName(root: ParentNode, name: string): HTMLButtonElement | null {
  return Array.from(root.querySelectorAll("button")).find((candidate) => {
    const label = candidate.getAttribute("aria-label") ?? candidate.textContent?.trim();
    return label === name;
  }) as HTMLButtonElement | null;
}

const planCalls: Array<[string, string | undefined, string | undefined]> = [];
const planHost = await render(
  <CodetermActionCard
    event={event(`codeterm plan task update ${PLAN} ${TASK}`)}
    expanded
    onToggle={() => {}}
    resolvePlanRef={(planId, focusId, focusKind) => ({ planId, focusId, focusKind, title: "Task", planTitle: "Plan", state: "resolved", rawLabel: TASK, fullRef: `${PLAN}/${TASK}` })}
    onOpenPlan={(planId, focusId, focusKind) => planCalls.push([planId, focusId, focusKind])}
  />,
);
const showTask = planHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-show-plan"]');
if (!showTask || showTask.textContent !== "Show Task" || showTask.textContent.includes(TASK) || showTask.textContent.includes(PLAN)) throw new Error("task navigation label regression");
await act(async () => showTask.click());
if (planCalls[0]?.join("|") !== `${PLAN}|${TASK}|task`) throw new Error("plan navigation callback regression");

const focusCalls: string[] = [];
const sendHost = await render(<CodetermActionCard event={event('codeterm send --tab e3fa0754 "hello"')} expanded onToggle={() => {}} resolveTabName={(ref) => ref === "e3fa0754" ? "Hugo" : undefined} onFocusTab={(ref) => focusCalls.push(ref)} />);
const focusTab = sendHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-focus-tab"]');
if (!focusTab || focusTab.textContent !== "Show Tab" || focusTab.textContent.includes("e3fa0754")) throw new Error("tab navigation label regression");
await act(async () => focusTab.click());
if (focusCalls[0] !== "e3fa0754") throw new Error("tab focus callback regression");
const meshId = "a35724e2-c7e3-486b-95d1-5edaa7f8d749";
const meshSendHost = await render(
  <CodetermActionCard
    event={event(`codeterm send "hello" --tab 166df18d --mesh ${meshId}`)}
    expanded={false}
    onToggle={() => {}}
    resolveTabName={(ref) => ref === "166df18d" ? "Dumas" : undefined}
    resolvePeerName={(ref) => ref === meshId ? "BLACKSTAR" : undefined}
  />,
);
const meshHeader = meshSendHost.host.querySelector("button")?.textContent ?? "";
if (!meshHeader.includes("Sent to") || !meshHeader.includes("Dumas @ BLACKSTAR") || meshHeader.includes(meshId)) throw new Error("mesh send header must use human endpoint names");
const successfulSendHost = await render(<CodetermActionCard event={{ ...event('codeterm send --tab e3fa0754 "hello"'), result: "OK (sent)\nOK" }} expanded onToggle={() => {}} />);
if (successfulSendHost.host.querySelector('[data-testid="toolcard-result"]') || successfulSendHost.host.textContent?.includes("OK (sent)")) throw new Error("successful send result must stay hidden");
const contradictorySendHost = await render(<CodetermActionCard event={{ ...event('codeterm send --tab e3fa0754 "hello"'), status: "error", result: 'Exit code: 2\nOK (sent)\nCODETERM_RESOLVED_TEXT="hello"' }} expanded onToggle={() => {}} />);
if (contradictorySendHost.host.querySelector('[data-testid="toolcard-result"]')) throw new Error("successful send must hide a contradictory wrapper exit code");
if (contradictorySendHost.host.querySelector('[data-testid="toolcard-status"]')?.getAttribute("aria-label") !== "success") throw new Error("successful send must override a contradictory wrapper status");
const failedSendHost = await render(<CodetermActionCard event={{ ...event('codeterm send --tab e3fa0754 "hello"'), status: "error", result: "error: tab not found" }} expanded onToggle={() => {}} />);
if (!failedSendHost.host.querySelector('[data-testid="toolcard-result"]') || !failedSendHost.host.textContent?.includes("tab not found")) throw new Error("failed send result must remain visible");
const resolvedSendHost = await render(<CodetermActionCard event={{ ...event('codeterm send --tab e3fa0754 "$VAR"'), result: 'OK (sent)\nCODETERM_RESOLVED_TEXT="expanded ***REDACTED*** text"' }} expanded onToggle={() => {}} />);
if (!resolvedSendHost.host.textContent.includes("expanded REDACTED text")) throw new Error("resolved send must render redacted text");
if (resolvedSendHost.host.textContent.includes("expanded-secret")) throw new Error("resolved send must not reveal secret text");
if (!resolvedSendHost.host.querySelector('[data-testid="raw-command-disclosure"]')) throw new Error("expanded card must own its raw command disclosure");
if (resolvedSendHost.host.querySelector('[data-testid="message-raw-content"]')) throw new Error("formatted card must hide the raw invocation");
const typedSendHost = await render(<CodetermActionCard event={event('codeterm send --tab e3fa0754 "$VAR"')} expanded onToggle={() => {}} />);
if (!typedSendHost.host.textContent?.includes("$VAR")) throw new Error("typed message body must render in the formatted card");
const padSendHost = await render(<CodetermActionCard event={event("codeterm send --tab e3fa0754 --from-pad message.md")} expanded onToggle={() => {}} />);
if (!padSendHost.host.textContent?.includes("message.md")) throw new Error("unresolved AgentPad send must retain its source descriptor");
const padRedirectHost = await render(<CodetermActionCard event={event("codeterm send --from-pad dumas-final-report.txt --tab 166df18d --mesh BLACKSTAR 2>&1")} expanded onToggle={() => {}} />);
if (!padRedirectHost.host.textContent?.includes("dumas-final-report.txt")) throw new Error("AgentPad card must preserve the source descriptor");
const mdSendHost = await render(<CodetermActionCard event={{ ...event("codeterm send --from-pad report.md --tab 166df18d --mesh BLACKSTAR"), result: 'OK (sent)\nCODETERM_RESOLVED_TEXT="# Status\\n\\n- one\\n- two"' }} expanded onToggle={() => {}} />);
if (!mdSendHost.host.textContent?.includes("Status") || !mdSendHost.host.textContent.includes("one") || mdSendHost.host.textContent.includes("# Status")) throw new Error("send with resolved markdown must render formatted content");
if (!mdSendHost.host.querySelector("h1") && !mdSendHost.host.querySelector("ul")) throw new Error("markdown send body must render block elements (h1/ul)");
const longResolvedBody = Array.from({ length: 12 }, (_, index) => `line ${index}`).join("\\n");
const persistentExpansionHost = await render(<CodetermActionCard event={{ ...event('codeterm send --tab e3fa0754 "long"'), result: `OK (sent)\nCODETERM_RESOLVED_TEXT="${longResolvedBody}"` }} expanded onToggle={() => {}} />);
const expandMessage = Array.from(persistentExpansionHost.host.querySelectorAll("button")).find((button) => button.textContent?.includes("more lines")) as HTMLButtonElement | undefined;
if (!expandMessage) throw new Error("long formatted send must retain the existing collapse affordance");
await act(async () => expandMessage.click());
if (!persistentExpansionHost.host.textContent?.includes("line 11")) throw new Error("long formatted send did not expand");
const sendHelpHost = await render(<CodetermActionCard event={event("codeterm send --help")} expanded={false} onToggle={() => {}} />);
if (!sendHelpHost.host.textContent?.includes("help") || !sendHelpHost.host.textContent?.includes("send") || sendHelpHost.host.textContent?.includes("current tab")) throw new Error("send help must not look like an outgoing message");
const docsHost = await render(<CodetermActionCard event={{ ...event("codeterm docs ORCHESTRATION"), result: "one two\nthree" }} expanded={false} onToggle={() => {}} />);
if (!docsHost.host.textContent?.includes("3 words") || docsHost.host.textContent.includes("chars")) throw new Error("docs metric must count words");
const unresolvedFocusHost = await render(<CodetermActionCard event={event('codeterm send --tab unknown-ref "hi"')} expanded onToggle={() => {}} onFocusTab={(ref) => focusCalls.push(ref)} />);
if (!unresolvedFocusHost.host.querySelector('[data-testid="codeterm-focus-tab"]')) throw new Error("tab focus should not require name resolver");

const closeHost = await render(<CodetermActionCard event={event("codeterm tab close tab_x99ab")} expanded={false} onToggle={() => {}} resolveTabName={(ref) => (ref === "tab_x99ab" ? "Research board" : undefined)} />);
if (!buttonByRoleName(closeHost.host, "Show full command")) throw new Error("close tab card missing accessible command toggle button");
if (!closeHost.host.textContent?.includes("tab close") || !closeHost.host.textContent.includes("Research board (tab_x99ab)")) throw new Error("close tab summary must include resolved label and id");
const closeStatus = closeHost.host.querySelector<HTMLSpanElement>('[data-testid="toolcard-status"]');
if (closeStatus?.getAttribute("aria-label") !== "success") throw new Error("close tab success event must render success icon");

const closeHostA = await render(<CodetermActionCard event={event("codeterm tab close tab_a12b")} expanded={false} onToggle={() => {}} resolveTabName={() => "Shared title"} />);
const closeHostB = await render(<CodetermActionCard event={event("codeterm tab close tab_c34d")} expanded={false} onToggle={() => {}} resolveTabName={() => "Shared title"} />);
if (!/Shared title \(tab_a12b\)/.test(closeHostA.host.textContent || "") || !/Shared title \(tab_c34d\)/.test(closeHostB.host.textContent || "")) throw new Error("close tab entries must stay distinguishable when titles collide");

const opened: string[] = [];
const navProps = {
  expanded: true,
  onToggle: () => {},
  onOpenPlan: (planId: string, focusId?: string, focusKind?: string) => opened.push(`${focusKind}:${planId}:${focusId ?? ""}`),
  resolvePlanRef: () => ({ title: "Target", planTitle: "Plan", state: "resolved" as const, rawLabel: "Target", fullRef: "Target" }),
};
const sectionHost = await render(<CodetermActionCard {...navProps} event={event(`codeterm plan section show ${PLAN} sec_tp5ov2`)} />);
const sectionButton = sectionHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-show-plan"]');
if (sectionButton?.textContent !== "Show Section") throw new Error("section navigation label regression");
await act(async () => sectionButton?.click());
if (opened.at(-1) !== `section:${PLAN}:sec_tp5ov2`) throw new Error("section navigation regression");

const specCalls: string[] = [];
const specHost = await render(<CodetermActionCard event={event("codeterm spec show sp_d17yp2")} expanded onToggle={() => {}} onOpenSpec={(id) => specCalls.push(id)} />);
const specButton = specHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-show-plan"]');
if (specButton?.textContent !== "Show Spec") throw new Error("spec navigation label regression");
await act(async () => specButton?.click());
if (specCalls[0] !== "sp_d17yp2") throw new Error("spec navigation regression");

const goalCalls: string[] = [];
const goalHost = await render(<CodetermActionCard {...navProps} event={event(`codeterm plan goal show ${PLAN}`)} onOpenPlan={(id, focus, kind) => goalCalls.push(`${id}:${focus ?? ""}:${kind ?? ""}`)} />);
const goalButton = goalHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-show-plan"]');
if (goalButton?.textContent !== "Show Goal") throw new Error("goal navigation label regression");
await act(async () => goalButton?.click());
if (goalCalls[0] !== `${PLAN}::plan`) throw new Error("goal navigation regression");

const memoryCalls: string[] = [];
const memoryHost = await render(<CodetermActionCard event={event("codeterm mem get mem_4orgnd")} expanded onToggle={() => {}} onOpenMemory={(id) => memoryCalls.push(id)} />);
const memoryButton = memoryHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-show-plan"]');
if (memoryButton?.textContent !== "Show Memory") throw new Error("memory navigation label regression");
await act(async () => memoryButton?.click());
if (memoryCalls[0] !== "mem_4orgnd") throw new Error("memory navigation regression");

const sourceCalls: string[] = [];
const sourceHost = await render(<CodetermActionCard event={event("codeterm mem get src_4orgnd")} expanded onToggle={() => {}} onOpenSource={(id) => sourceCalls.push(id)} />);
const sourceButton = sourceHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-show-plan"]');
if (sourceButton?.textContent !== "Show Source") throw new Error("source navigation label regression");
await act(async () => sourceButton?.click());
if (sourceCalls[0] !== "src_4orgnd") throw new Error("source navigation regression");
const specSectionHost = await render(<CodetermActionCard event={event("codeterm spec section show sp_d17yp2 requirements")} expanded={false} onToggle={() => {}} onOpenSpec={(id) => specCalls.push(id)} />);
if (!specSectionHost.host.textContent?.includes("spec §") || specSectionHost.host.textContent.includes("sp_d17yp2")) throw new Error("spec section descriptor regression");

const noTargetHost = await render(<CodetermActionCard event={event("codeterm mem get not-a-ref")} expanded onToggle={() => {}} onOpenMemory={() => {}} />);
if (noTargetHost.host.querySelector("button[data-testid^='codeterm-']")) throw new Error("dead navigation button regression");

const descriptorCases: Array<[string, string]> = [
  ["codeterm plan create --title \"New plan\"", "plan"],
  ["codeterm plan goal set pl_d17yp2 --vector \"North star\"", "goal"],
  ["codeterm spec create --title \"Requirements\"", "spec"],
  ["codeterm plan section add pl_d17yp2 --handle contract", "section"],
  ["codeterm plan task add pl_d17yp2 --name \"Implement card\"", "plan task"],
  ["codeterm plan record pl_d17yp2 --task task_4orgnd --step \"decision\"", "decision"],
  ["codeterm agent report --status in_review --sha abc123", "report"],
  ["codeterm send --tab e3fa0754 \"hello\"", "Sent to"],
  ["codeterm mem save --kind fact \"remember this\"", "mem save"],
  ["codeterm tab list", "tab list"],
  ["codeterm workspace focus ws_codeterm", "workspace"],
  ["codeterm cron list", "crons"],
  ["codeterm keepawake status", "status"],
  ["codeterm docs ORCHESTRATION", "docs"],
  ["codeterm plugin list", "plugins"],
];
for (const [command, lead] of descriptorCases) {
  const descriptorHost = await render(<CodetermActionCard event={event(command)} expanded={false} onToggle={() => {}} />);
  if (!descriptorHost.host.textContent?.includes(lead) || descriptorHost.host.textContent.includes(command)) throw new Error(`descriptor regression: ${lead}`);
}
const collapsedHost = await render(<CodetermActionCard event={event("codeterm plan task add pl_d17yp2 --name Implement")} expanded={false} onToggle={() => {}} />);
if (collapsedHost.host.querySelector('[data-testid="toolcard-command"]')) throw new Error("raw command leaked while collapsed");
if (collapsedHost.host.querySelector('[data-testid="toolcard-command-open"]')) throw new Error("per-card command opener must not render");
const chipHost = await render(<CodetermActionCard event={event("codeterm agent spawn codex --model gpt-5.6-luna")} expanded={false} onToggle={() => {}} />);
if (!chipHost.host.textContent?.includes("gpt-5.6-luna") || chipHost.host.querySelector('[data-testid="toolcard-command"]')) throw new Error("collapsed chip regression");

const reportCard = await render(<CodetermActionCard event={event('codeterm agent report --status in_review --sha 979a8683e --text "STATUS: in_review\nCHANGES: Added cards\nTESTS RUN: 12 passed"')} expanded onToggle={() => {}} />);
if (!reportCard.host.textContent?.includes("in_review") || !reportCard.host.textContent.includes("979a8683e")) throw new Error("report header regression");
if (!reportCard.host.textContent.includes("changes") || !reportCard.host.textContent.includes("Added cards") || !reportCard.host.textContent.includes("tests") || !reportCard.host.textContent.includes("12 passed")) throw new Error("report fields regression");

const tabOutputHost = await render(<CodetermActionCard event={event("codeterm tab get-text --tab e3fa0754")} expanded={false} onToggle={() => {}} resolveTabName={() => "Hugo"} />);
if (!tabOutputHost.host.textContent?.includes("tab get-text") || !tabOutputHost.host.textContent.includes("Hugo")) throw new Error("tab get-text descriptor regression");
if (!tabOutputHost.host.querySelector(".lucide-scroll-text") || tabOutputHost.host.querySelector(".lucide-terminal")) throw new Error("tab get-text icon must differ from shell command");
const wrappedTabList = await render(<CodetermActionCard event={{
  ...event("codeterm tab list"),
  result: `Exit code: 0\nOutput:\n${JSON.stringify([{ id: "one" }, { id: "two" }, { id: "three" }], null, 2)}`,
}} expanded={false} onToggle={() => {}} />);
if (!wrappedTabList.host.textContent?.includes("3 tabs") || wrappedTabList.host.textContent.includes("11 tabs")) throw new Error("wrapped tab list must count JSON entries instead of output lines");
const compactTabList = await render(<CodetermActionCard event={{
  ...event("codeterm tab list"),
  result: "General  ws_general\n  aaaa1111 Adele - Working\n\nResearch  ws_research\n  bbbb2222 Scout - Idle",
}} expanded onToggle={() => {}} />);
if (!compactTabList.host.querySelector('[data-testid="tab-list-compact"]') || compactTabList.host.querySelector('[data-testid="toolcard-result"]')) throw new Error("tab list must render compact output by default");
if (!compactTabList.host.textContent?.includes("Adele") || !compactTabList.host.textContent.includes("Working")) throw new Error("compact tab list labels regression");
const jsonTabList = await render(<CodetermActionCard event={{
  ...event("codeterm tab list --json"),
  result: '{"tabs":[{"id":"aaaa1111","title":"Adele"},{"id":"bbbb2222","title":"Scout"}]}',
}} expanded onToggle={() => {}} />);
if (!jsonTabList.host.querySelector('[data-testid="tab-list-json"]') || jsonTabList.host.querySelector('[data-testid="tab-list-compact"]') || !jsonTabList.host.textContent?.includes("JSON inspection")) throw new Error("tab list JSON must remain full inspection output");
if (!jsonTabList.host.textContent?.includes("2 tabs")) throw new Error("tab list --json count must read the tabs key of the shipped envelope");
const meshTabs = await render(<CodetermActionCard event={event("codeterm mesh tabs peer-a")} expanded onToggle={() => {}} />);
if (meshTabs.host.querySelector('[data-testid="tab-list-compact"]')) throw new Error("mesh tabs must not use the local tab list renderer");

const catalogVisuals: Array<[string, string]> = [
  ['codeterm plan create --title "Plan"', "lucide-map"],
  ["codeterm plan goal set pl_d17yp2", "lucide-git-branch"],
  ['codeterm spec create --title "Spec"', "lucide-file-plus-corner"],
  ["codeterm spec section add sp_d17yp2 --title Requirements", "lucide-file-text"],
  ["codeterm spec link add pl_d17yp2 --spec sp_d17yp2", "lucide-link-2"],
  ["codeterm plan section add pl_d17yp2 --title Wave", "lucide-file-plus-corner"],
  ["codeterm plan section update pl_d17yp2 wave", "lucide-file-pen-line"],
  ["codeterm plan task add pl_d17yp2 --name Work", "lucide-list-checks"],
  ["codeterm plan task update task_4orgnd --status done", "lucide-file-pen-line"],
  ["codeterm plan record pl_d17yp2 --step decision", "lucide-message-square-text"],
  ["codeterm plan retrospective pl_d17yp2", "lucide-history"],
  ["codeterm plan skill new verify", "lucide-sparkles"],
  ["codeterm plan state pl_d17yp2", "lucide-activity"],
  ["codeterm agent spawn codex --role Worker", "lucide-bot"],
  ["codeterm agent report --status in_review", "lucide-message-square-text"],
  ["codeterm orchestrator status", "lucide-users"],
  ['codeterm send --tab e3fa0754 "hi"', "lucide-send"],
  ['codeterm send --mesh host "go"', "lucide-radio-tower"],
  ["codeterm mesh peers", "lucide-network"],
  ['codeterm mem save "fact"', "lucide-brain"],
  ['codeterm mem search "query"', "lucide-search"],
  ["codeterm mem wake-up", "lucide-sunrise"],
  ["codeterm tab new --cmd dev", "lucide-square-plus"],
  ["codeterm tab list", "lucide-list-tree"],
  ["codeterm workspace focus ws", "lucide-folder-kanban"],
  ["codeterm cron create --schedule daily", "lucide-clock-3"],
  ["codeterm cron list", "lucide-calendar-clock"],
  ["codeterm keepawake start", "lucide-coffee"],
  ["codeterm docs ORCHESTRATION", "lucide-book-open"],
  ["codeterm plugin list", "lucide-puzzle"],
];
for (const [command, iconClass] of catalogVisuals) {
  const host = await render(<CodetermActionCard event={event(command)} expanded={false} onToggle={() => {}} />);
  if (!host.host.querySelector(`.${iconClass}`)) throw new Error(`catalog visual regression: ${command} -> ${iconClass}`);
}

const unknownHost = await render(<CodetermActionCard event={event("codeterm mem search hello")} expanded onToggle={() => {}} />);
if (unknownHost.host.querySelector("button[data-testid^='codeterm-']")) throw new Error("unexpected navigation button");

const themeVars = themeToCSS(DEFAULT_THEME);
for (const variable of ["--ct-fg", "--ct-muted-fg", "--ct-accent", "--ct-border"]) if (!(variable in themeVars)) throw new Error(`missing theme token ${variable}`);
console.log("codeterm action card tests: 27/27 passed");
