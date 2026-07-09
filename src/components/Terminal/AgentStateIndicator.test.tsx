// Run: npx tsx src/components/Terminal/AgentStateIndicator.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { AgentStateIndicator } from "./AgentStateIndicator";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div") as unknown as HTMLDivElement;
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

test("working line enriches status with provider and model", async () => {
  const { host } = await render(
    <AgentStateIndicator
      state={{
        state: "working",
        status_line: "Channeling… (1s · ↓21 tokens)",
      }}
      providerName="cursor"
      model="gpt-5.5"
    />,
  );
  const row = host.querySelector('[data-testid="agent-state"]');
  assert(row?.getAttribute("data-agent-state") === "working", "working state");
  const msg = host.querySelector(".ct-agent-msg");
  assert(
    msg?.textContent === "cursor · gpt-5.5 · Channeling… (1s · ↓21 tokens)",
    `expected enriched working line, got "${msg?.textContent}"`,
  );
});

test("working line without provider/model shows status_line only", async () => {
  const { host } = await render(
    <AgentStateIndicator
      state={{
        state: "working",
        status_line: "Editing 36.86k tokens",
      }}
    />,
  );
  const msg = host.querySelector(".ct-agent-msg");
  assert(msg?.textContent === "Editing 36.86k tokens", "bare status line when no provider metadata");
});

test("summary line is prefixed with provider and model", async () => {
  const { host } = await render(
    <AgentStateIndicator
      state={{
        state: "idle",
        last_turn_summary: "Done — 3 files changed",
      }}
      providerName="Claude Code"
      model="opus"
    />,
  );
  const msg = host.querySelector(".ct-agent-msg");
  assert(
    msg?.textContent === "Claude Code · opus · Done — 3 files changed",
    `summary carries who and what: got "${msg?.textContent}"`,
  );
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}: ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n✗ ${failed}/${tests.length} AgentStateIndicator tests failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} AgentStateIndicator tests green`);
}

void main();
