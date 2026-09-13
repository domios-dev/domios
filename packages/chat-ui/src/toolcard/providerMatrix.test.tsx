import React from "react";
import type { ToolEvent } from "@codeterm/chat-core";
import { ToolCallBubble } from "../ToolCallBubble";
import { act, event, render } from "./cardTestSetup";

function command(detail: string, result?: string): ToolEvent {
  return event({ kind: "command", status: "success", summary: detail.split(/\r?\n/).at(-1) ?? detail, detail, result });
}

const providerCommands = [
  ["claude", "cd /d/Developer/codeterm\ncodeterm docs ORCHESTRATION"],
  ["codex", "cd D:\\Developer\\codeterm && codeterm docs ORCHESTRATION"],
  ["opencode", "Set-Location 'D:\\Developer\\codeterm'; codeterm docs ORCHESTRATION"],
  ["cursor", "codeterm docs ORCHESTRATION"],
] as const;

for (const [provider, detail] of providerCommands) {
  const host = await render(<ToolCallBubble event={command(detail, "document body")} expanded={false} onToggle={() => {}} />);
  if (!host.querySelector(".lucide-book-open") || !host.textContent?.includes("docs") || !host.textContent.includes("ORCHESTRATION")) {
    throw new Error(`${provider} docs command did not use the special docs card`);
  }
  if (host.textContent.includes("command")) throw new Error(`${provider} docs command fell back to CommandCard`);
}

const report = 'cd /d/Developer/codeterm\ncodeterm agent report --status in_review --sha 979a8683e --text "STATUS: in_review\nCHANGES: Added cards\nTESTS RUN: 12 passed" 2>&1 | tail -3';
const reportHost = await render(<ToolCallBubble event={command(report, "ok true")} expanded onToggle={() => {}} />);
const reportGroups = reportHost.querySelectorAll('[data-testid="chat-command-group"]');
if (reportGroups.length !== 1 || reportGroups[0].querySelectorAll('[data-testid="chat-command-group-row"]').length !== 3) {
  throw new Error("piped Claude agent report did not render one grouped box with a row per segment");
}
const reportRow = reportGroups[0].querySelector('[data-row-role="codeterm"]');
if (!reportRow?.querySelector(".lucide-message-square-text") || !reportRow.textContent?.includes("report")) {
  throw new Error("Claude agent report lost its CodeTerm card inside the group");
}
await act(async () => reportRow.querySelector<HTMLButtonElement>('button[aria-expanded="false"]')?.click());
if (!reportRow.textContent?.includes("979a8683e") || !reportRow.textContent.includes("Added cards") || !reportRow.textContent.includes("12 passed")) {
  throw new Error("agent report fields were not rendered");
}

const plainHost = await render(<ToolCallBubble event={command("git status --short")} expanded={false} onToggle={() => {}} />);
if (!plainHost.querySelector(".lucide-terminal") || plainHost.querySelector(".lucide-brain")) throw new Error("non-action command was misclassified");
const pipedSaveHost = await render(<ToolCallBubble event={command('cat notes.txt | codeterm mem save "payload"')} expanded={false} onToggle={() => {}} />);
const pipedRoles = [...pipedSaveHost.querySelectorAll<HTMLElement>('[data-testid="chat-command-group-row"]')].map((row) => row.dataset.rowRole);
if (pipedRoles.join() !== "command,codeterm" || !pipedSaveHost.querySelector('[data-row-role="command"] .lucide-terminal') || !pipedSaveHost.querySelector('[data-row-role="codeterm"] .lucide-brain')) {
  throw new Error("a pipeline into a CodeTerm action must group a command row and a CodeTerm row");
}

// Piping into a reader still reads; only a pipeline with a non-reader stage
// falls back to the plain command card.
for (const [detail, icon] of [["rg TODO src | head -20", ".lucide-search"], ["cat notes.txt | head -40", ".lucide-file-search"]] as const) {
  const host = await render(<ToolCallBubble event={command(detail)} expanded={false} onToggle={() => {}} />);
  if (!host.querySelector(icon)) throw new Error(`reader-only pipeline lost its card: ${detail}`);
}

for (const detail of ["runner opaque-input\n\ncodeterm agent spawn --help", "codeterm agent spawn --help\n\nrunner opaque-input", "codeterm send --help; codeterm docs ROUTING"]) {
  const mixed = event({ kind: "command", summary: detail, detail });
  const host = await render(<ToolCallBubble event={mixed} expanded={false} onToggle={() => {}} />);
  const groups = host.querySelectorAll('[data-testid="chat-command-group"]');
  if (groups.length !== 1 || groups[0].querySelectorAll('[data-testid="chat-command-group-row"]').length !== 2) {
    throw new Error("mixed command batch was not split into one grouped box");
  }
  if (host.querySelectorAll('[data-testid="chat-command-group-operator"]').length !== 1) throw new Error("batch rows lost their connector");
}

console.log("provider-neutral CodeTerm action matrix passed");
