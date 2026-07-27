import React, { useState } from "react";
import { ToolCallBubble } from "../ToolCallBubble";
import { ShowCommandsProvider } from "./toolKindVisual";
import type { ToolEvent, ToolEventKind } from "@codeterm/chat-core";
import { act, event, render } from "./cardTestSetup";

function tool(kind: ToolEventKind, detail = `${kind} detail`): ToolEvent {
  return event({ kind, summary: `${kind} summary`, detail });
}

const expectedLabels: Record<ToolEventKind, string> = {
  command: "command",
  edit: "edit",
  read: "read",
  search: "search",
  task: "task",
  fetch: "fetch",
  codeterm: "send",
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

const commandEvent = tool("command", "echo hidden");
function ToggleProvider() {
  const [showCommands, setShowCommands] = useState(false);
  return (
    <ShowCommandsProvider showCommands={showCommands}>
      <button type="button" data-testid="toggle-commands" onClick={() => setShowCommands((visible) => !visible)}>Toggle</button>
      <ToolCallBubble event={commandEvent} expanded={false} onToggle={() => {}} />
      <ToolCallBubble event={{ ...commandEvent, id: "second" }} expanded={false} onToggle={() => {}} />
    </ShowCommandsProvider>
  );
}

const globalHost = await render(
  <ToggleProvider />,
);
if (globalHost.querySelectorAll('[data-testid="toolcard-command"]').length !== 0) throw new Error("commands were not hidden initially");
const toggle = globalHost.querySelector<HTMLButtonElement>('[data-testid="toggle-commands"]');
if (!toggle) throw new Error("global toggle missing");
await act(async () => toggle.click());
if (globalHost.querySelectorAll('[data-testid="toolcard-command"]').length !== 2) throw new Error("global command toggle did not reveal all commands");

console.log("tool card dispatch tests: 12 assertions passed");
