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

// Piping into a reader still reads; only a pipeline with a non-reader stage
// falls back to the plain command card.
for (const [detail, icon] of [["rg TODO src | head -20", ".lucide-search"], ["cat notes.txt | head -40", ".lucide-file-search"]] as const) {
  const host = await render(<ToolCallBubble event={command(detail)} expanded={false} onToggle={() => {}} />);
  if (!host.querySelector(icon)) throw new Error(`reader-only pipeline lost its card: ${detail}`);
}

for (const detail of ["runner opaque-input\n\ncodeterm agent spawn --help", "codeterm agent spawn --help\n\nrunner opaque-input", "codeterm send --help; codeterm docs ROUTING"]) {
  const mixed = event({ kind: "command", summary: detail, detail });
  const host = await render(<ToolCallBubble event={mixed} expanded={false} onToggle={() => {}} />);
  if (!host.querySelector(".lucide-terminal") || host.querySelector(".lucide-book-open")) {
    throw new Error("mixed command batch was reduced to its help action");
  }
  if (!host.textContent?.includes(mixed.summary ?? "")) throw new Error("batch command summary was hidden");
}

console.log("provider-neutral CodeTerm action matrix passed");
