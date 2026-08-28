import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { CommandCard } from "./CommandCard";
import { EditCard } from "./EditCard";
import { GenericToolCard } from "./GenericToolCard";
import type { ToolEvent } from "@codeterm/chat-core";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function event(overrides: Partial<ToolEvent> = {}): ToolEvent {
  return { id: "id", phase: "call", status: "pending", kind: "command", summary: "codeterm agent reports ...", message: { id: "id", type: "tool_call", content: "" }, ...overrides };
}

async function render(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, root };
}

const cases: Array<[string, () => Promise<void>]> = [
  ["command hides raw JSON and shows pending spinner", async () => {
    const { host } = await render(<CommandCard event={event({ detail: "codeterm agent reports the full command" })} expanded onToggle={() => {}} />);
    if (!host.textContent?.includes("codeterm agent reports the full command")) throw new Error("command_card_reveal expanded command regression");
    if (!host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("pending command spinner regression");
    if (host.textContent.includes('{"raw')) throw new Error("raw tool JSON leak regression");
    if (host.querySelector('[data-testid="toolcard-command-open"]')) throw new Error("per-card command opener must not render");
  }],
  ["edit shows counts and diff", async () => {
    const { host } = await render(<EditCard event={event({ kind: "edit", edits: [{ path: "src/x.ts", old: "const value = 1;", new: "const value = 2;" }] })} expanded onToggle={() => {}} />);
    const highlightedKeyword = Array.from(host.querySelectorAll("span")).some((node) => node.textContent === "const");
    if (!host.textContent?.includes("src/x.ts +1 -1") || !host.textContent.includes("- const value = 1;") || !host.textContent.includes("+ const value = 2;") || !highlightedKeyword) throw new Error("edit card regression");
  }],
  ["error renders dashed card and result", async () => {
    const { host } = await render(<GenericToolCard event={event({ kind: "search", phase: "result", status: "error", result: "RESULT" })} expanded onToggle={() => {}} />);
    if (!host.textContent?.includes("RESULT") || !host.querySelector('[style*="dashed"]')) throw new Error("error lifecycle regression");
  }],
  ["long output stays bounded and scrollable", async () => {
    const { host } = await render(<GenericToolCard event={event({ kind: "fetch", phase: "result", status: "success", result: "line\n".repeat(500) })} expanded onToggle={() => {}} />);
    const output = host.querySelector('[data-testid="toolcard-result"] pre');
    if (!output || !output.className.includes("overflow-auto") || (output as HTMLElement).style.maxHeight !== "12rem") throw new Error("bounded output regression");
  }],
  ["generic does not leak braces", async () => {
    const { host } = await render(<GenericToolCard event={event({ kind: "generic", detail: "plain" })} expanded onToggle={() => {}} />);
    if (host.textContent?.includes("{")) throw new Error("generic JSON leak");
  }],
];

for (const [name, run] of cases) {
  await run();
  console.log(`✓ ${name}`);
}
console.log(`toolcard tests: ${cases.length}/${cases.length} passed`);
