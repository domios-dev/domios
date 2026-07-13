// Run: npx tsx src/components/AgentRun/ToolOutputCard.test.tsx
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { stringifyToolPayload, ToolOutputCard } from "./ToolOutputCard";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
const test = (name: string, fn: TestFn) => tests.push([name, fn]);
const assert: (value: unknown, message: string) => asserts value = (value, message) => {
  if (!value) throw new Error(message);
};

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

setupDom();

test("normalizes exec/wait, SSH, mesh, and plugin envelopes without losing metadata", () => {
  const samples = [
    "exec result",
    { output: "wait result" },
    { text: "ssh result" },
    { content: "mesh result" },
    { result: "plugin result" },
  ];
  const expected = ["exec result", "wait result", "ssh result", "mesh result", "plugin result"];
  const normalized = samples.map(stringifyToolPayload);
  assert(normalized[0] === "exec result", "plain exec result should remain exact");
  assert(normalized.slice(1).every((payload, index) => payload.includes(expected[index + 1])), "transport output text should remain present");
  assert(normalized[1].includes('"output"'), "wait envelope metadata should remain present");
});

test("long collapsed payload has an explicit two-line preview and accessible expand", async () => {
  const payload = `{"value":"${"x".repeat(600)}"}`;
  const { host, root } = await render(<ToolOutputCard kind="call" toolName="exec" payload={payload} />);
  const preview = host.querySelector('[data-testid="tool-output-preview"]') as HTMLElement | null;
  const expand = host.querySelector('button[aria-label="Expand full exec call"]') as HTMLButtonElement | null;
  assert(preview?.classList.contains("line-clamp-2"), "preview must be constrained to two visual lines");
  assert(preview?.style.overflow === "hidden", "preview must not overflow the card");
  assert(preview?.style.overflowWrap === "anywhere", "unbroken JSON must wrap inside its card");
  assert(expand?.getAttribute("aria-expanded") === "false", "expand must expose collapsed state");
  await act(async () => expand?.click());
  assert(host.querySelector('[data-testid="tool-output-full"]')?.textContent === payload, "expanded output must equal original payload");
  assert(host.querySelector('button[aria-label="Collapse exec call"]')?.getAttribute("aria-expanded") === "true", "collapse must expose expanded state");
  await act(async () => root.unmount());
});

test("copy full output uses the exact stored payload and error status stays outside the body", async () => {
  const payload = `{"timeout":true,"detail":"${"unbroken".repeat(80)}"}`;
  let copied = "";
  Object.defineProperty(navigator, "clipboard", { value: { writeText: async (value: string) => { copied = value; } }, configurable: true });
  const { host, root } = await render(<ToolOutputCard kind="result" payload={payload} isError />);
  const expand = host.querySelector('button[aria-label="Expand full Error output"]') as HTMLButtonElement;
  await act(async () => expand.click());
  const copy = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Copy full output");
  assert(copy, "expanded card must expose Copy full output");
  await act(async () => copy.click());
  assert(copied === payload, "clipboard payload must not be truncated or reformatted");
  assert(host.querySelector('[data-testid="tool-output-status"]')?.textContent === "failed", "error metadata must remain outside collapsed body");
  await act(async () => root.unmount());
});

test("copy button precedes collapse in keyboard order", async () => {
  const payload = `{"value":"${"x".repeat(600)}"}`;
  const { host, root } = await render(<ToolOutputCard kind="call" toolName="exec" payload={payload} />);
  const expand = host.querySelector('button[aria-label="Expand full exec call"]') as HTMLButtonElement;
  await act(async () => expand.click());
  const buttons = Array.from(host.querySelectorAll("button")).map((button) => button.textContent?.trim());
  assert(buttons[0] === "Copy full output", "copy must be first in expanded keyboard order");
  assert(buttons[1] === "Collapse ▾", "collapse must follow copy for Tab then Enter");
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try { await fn(); console.log(`✓ ${name}`); }
    catch (error) { failed += 1; console.error(`✗ ${name}`, error); }
  }
  if (failed) throw new Error(`${failed} ToolOutputCard test(s) failed`);
}

void main();
