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
const sendHost = await render(<CodetermActionCard event={event('codeterm send --tab e3fa0754 "hello"')} expanded onToggle={() => {}} resolvePaneName={(ref) => ref === "e3fa0754" ? "Hugo" : undefined} onFocusPane={(ref) => focusCalls.push(ref)} />);
const focusTab = sendHost.host.querySelector<HTMLButtonElement>('[data-testid="codeterm-focus-tab"]');
if (!focusTab || focusTab.textContent !== "Focus Tab" || focusTab.textContent.includes("e3fa0754")) throw new Error("tab navigation label regression");
await act(async () => focusTab.click());
if (focusCalls[0] !== "e3fa0754") throw new Error("tab focus callback regression");
const unresolvedFocusHost = await render(<CodetermActionCard event={event('codeterm send --tab unknown-ref "hi"')} expanded onToggle={() => {}} onFocusPane={(ref) => focusCalls.push(ref)} />);
if (!unresolvedFocusHost.host.querySelector('[data-testid="codeterm-focus-tab"]')) throw new Error("tab focus should not require name resolver");

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
const goalHost = await render(<CodetermActionCard {...navProps} event={event(`codeterm plan goal get ${PLAN}`)} onOpenPlan={(id, focus, kind) => goalCalls.push(`${id}:${focus ?? ""}:${kind ?? ""}`)} />);
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
  ["codeterm plan record pl_d17yp2 --task task_4orgnd --step \"decision\"", "plan"],
  ["codeterm agent report --status in_review --sha abc123", "agent report"],
  ["codeterm send --tab e3fa0754 \"hello\"", "send"],
  ["codeterm mem save --kind fact \"remember this\"", "mem save"],
  ["codeterm tab list", "tab list"],
  ["codeterm workspace focus ws_codeterm", "workspace focus"],
  ["codeterm cron list", "cron list"],
  ["codeterm keepawake status", "keepawake status"],
  ["codeterm docs ORCHESTRATION", "docs"],
  ["codeterm plugin list", "plugin list"],
];
for (const [command, lead] of descriptorCases) {
  const descriptorHost = await render(<CodetermActionCard event={event(command)} expanded={false} onToggle={() => {}} />);
  if (!descriptorHost.host.textContent?.includes(lead) || descriptorHost.host.textContent.includes(command)) throw new Error(`descriptor regression: ${lead}`);
}
const collapsedHost = await render(<CodetermActionCard event={event("codeterm plan task add pl_d17yp2 --name Implement")} expanded={false} onToggle={() => {}} />);
if (collapsedHost.host.querySelector('[data-testid="toolcard-command"]')) throw new Error("raw command leaked while collapsed");
const commandOpener = collapsedHost.host.querySelector<HTMLButtonElement>('[data-testid="toolcard-command-open"]');
if (!commandOpener) throw new Error("action command opener missing");
await act(async () => commandOpener.click());
if (!collapsedHost.host.querySelector('[data-testid="toolcard-command"]')?.textContent?.includes("codeterm plan task add")) throw new Error("raw command opener regression");
const chipHost = await render(<CodetermActionCard event={event("codeterm agent spawn codex --model gpt-5.6-luna")} expanded={false} onToggle={() => {}} />);
if (!chipHost.host.textContent?.includes("gpt-5.6-luna") || chipHost.host.querySelector('[data-testid="toolcard-command"]')) throw new Error("collapsed chip regression");

const unknownHost = await render(<CodetermActionCard event={event("codeterm mem search hello")} expanded onToggle={() => {}} />);
if (unknownHost.host.querySelector("button[data-testid^='codeterm-']")) throw new Error("unexpected navigation button");

const themeVars = themeToCSS(DEFAULT_THEME);
for (const variable of ["--ct-fg", "--ct-muted-fg", "--ct-accent", "--ct-border"]) if (!(variable in themeVars)) throw new Error(`missing theme token ${variable}`);
console.log("codeterm action card tests: 26/26 passed");
