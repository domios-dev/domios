import React from "react";
import type { ToolEvent } from "@codeterm/chat-core";
import { ToolCallBubble } from "../ToolCallBubble";
import { event, render } from "./cardTestSetup";

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
if (!reportHost.querySelector(".lucide-message-square-text") || !reportHost.textContent?.includes("report") || !reportHost.textContent.includes("979a8683e")) {
  throw new Error("Claude agent report did not use the special report card");
}
if (!reportHost.textContent.includes("Added cards") || !reportHost.textContent.includes("12 passed")) {
  throw new Error("agent report fields were not rendered");
}

for (const detail of ["git status --short", 'cat notes.txt | codeterm mem save "payload"']) {
  const host = await render(<ToolCallBubble event={command(detail)} expanded={false} onToggle={() => {}} />);
  if (!host.querySelector(".lucide-terminal") || host.querySelector(".lucide-brain")) throw new Error(`non-action command was misclassified: ${detail}`);
}

console.log("provider-neutral CodeTerm action matrix: 7/7 passed");
