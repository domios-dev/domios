import React from "react";
import { ToolCallBubble } from "../ToolCallBubble";
import type { ToolEvent, ToolEventKind } from "@codeterm/chat-core";
import { act, event, render } from "./cardTestSetup";

function tool(kind: ToolEventKind, detail = `${kind} detail`): ToolEvent {
  return event({ kind, summary: `${kind} summary`, detail });
}

const expectedLabels: Record<ToolEventKind, string> = {
  command: "command",
  edit: "edit",
  read: "Read",
  search: "Search",
  task: "task",
  fetch: "fetch",
  codeterm: "Sent to",
  web: "web",
  image: "image",
  generic: "generic summary",
};

for (const kind of Object.keys(expectedLabels) as ToolEventKind[]) {
  const detail = kind === "codeterm" ? 'codeterm send --tab e3fa0754 "hello"' : undefined;
  const host = await render(<ToolCallBubble event={tool(kind, detail)} expanded={false} onToggle={() => {}} />);
  if (!host.textContent?.includes(expectedLabels[kind])) throw new Error(`${kind} dispatched to the wrong card`);
}

const codetermHost = await render(
  <ToolCallBubble event={tool("command", 'codeterm send --tab e3fa0754 "hello"')} expanded onToggle={() => {}} onFocusTab={() => {}} />,
);
if (!codetermHost.querySelector('[data-testid="codeterm-focus-tab"]')) throw new Error("recognized codeterm command did not use CodetermActionCard");

const focusCalls: string[] = [];
const resolverHost = await render(
  <ToolCallBubble
    event={tool("codeterm", 'codeterm send --tab e3fa0754 "hello"')}
    expanded
    onToggle={() => {}}
    resolveTabName={(ref) => ref === "e3fa0754" ? "Hugo" : undefined}
    onFocusTab={(ref) => focusCalls.push(ref)}
  />,
);
const focusButton = resolverHost.querySelector<HTMLButtonElement>('[data-testid="codeterm-focus-tab"]');
if (!focusButton) throw new Error("CodetermActionCard did not receive the tab resolver");
await act(async () => focusButton.click());
if (focusCalls.join() !== "e3fa0754") throw new Error("tab focus resolver did not fire");
if (!resolverHost.textContent?.includes("Hugo")) throw new Error("tab-name resolver did not reach the CodetermActionCard");

const searchHost = await render(
  <ToolCallBubble event={tool("command", 'rg -n "needle" src')} expanded={false} onToggle={() => {}} />,
);
if (!searchHost.textContent?.includes("Search")) throw new Error("rg command did not use SearchCard");

const readHost = await render(
  <ToolCallBubble event={tool("command", "sed -n '1,80p' src/main.ts && tail -20 src/lib.ts")} expanded={false} onToggle={() => {}} />,
);
if (!readHost.textContent?.includes("Read") || !readHost.textContent.includes("main.ts, lib.ts")) throw new Error("read-only shell pipeline did not use a named ReadCard");

console.log("tool card dispatch tests: 11 assertions passed");
