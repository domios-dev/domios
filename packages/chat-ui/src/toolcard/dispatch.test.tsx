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

const fileDiff = (path: string, additions: number, deletions: number) => ({
  path, additions, deletions,
  hunks: [{ oldStart: 1, oldLines: deletions, newStart: 1, newLines: additions, lines: [...Array<string>(deletions).fill("-old"), ...Array<string>(additions).fill("+new")] }],
});
const diffHost = await render(
  <ToolCallBubble
    event={event({ kind: "edit", status: "success", summary: "src/a.ts", edits: [{ path: "src/a.ts", old: "x\ny\nz", new: "q" }], fileDiffs: [fileDiff("src/a.ts", 6, 5), fileDiff("src/b.js", 1, 0)] })}
    expanded
    onToggle={() => {}}
  />,
);
const diffHeaders = [...diffHost.querySelectorAll('[data-testid="file-diff-header"]')].map((node) => node.textContent);
if (diffHeaders.join("|") !== "src/a.ts +6 −5|src/b.js +1 −0") throw new Error("diff card counts must come from fileDiffs");
if (diffHost.querySelectorAll('[data-testid="file-diff-card"]').length !== 2) throw new Error("an edit with fileDiffs must render one card per file");
if (diffHost.querySelector('[data-testid="toolcard-edit-file"], [data-testid="toolcard-edit-summary"], [data-testid="chat-tool-call"]')) throw new Error("an edit with fileDiffs must not render the old block diff card");
const legacyEditHost = await render(
  <ToolCallBubble event={event({ kind: "edit", status: "success", edits: [{ path: "src/a.ts", old: "old", new: "new" }] })} expanded onToggle={() => {}} />,
);
if (!legacyEditHost.querySelector('[data-testid="toolcard-edit-summary"]') || legacyEditHost.querySelector('[data-testid="file-diff-card"]')) throw new Error("an edit without fileDiffs must keep the edit card");

console.log("tool card dispatch tests: passed");
