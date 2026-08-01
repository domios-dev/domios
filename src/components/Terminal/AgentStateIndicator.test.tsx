// Run: npx tsx src/components/Terminal/AgentStateIndicator.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { AgentStateIndicator, shouldRenderAgentStateIndicator } from "./AgentStateIndicator";

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

test("question chrome requires an authoritative actionable prompt", () => {
  assert(
    !shouldRenderAgentStateIndicator("question", true, false),
    "diagnostic question state alone never renders Waiting for you",
  );
  assert(
    shouldRenderAgentStateIndicator("question", false, true),
    "an authoritative question renders even before transcript messages",
  );
  assert(
    shouldRenderAgentStateIndicator("working", true, false),
    "ordinary working status still renders with transcript messages",
  );
});

test("question close is accessible and only dismisses the current generation", async () => {
  let cancelCalls = 0;
  let stopCalls = 0;
  let terminalCalls = 0;
  const { host, root } = await render(
    <AgentStateIndicator
      state={{ state: "question" }}
      questionGeneration={1}
      onCancel={() => { cancelCalls += 1; }}
      onStop={() => { stopCalls += 1; }}
      onOpenTerminal={() => { terminalCalls += 1; }}
    />,
  );

  const close = host.querySelector<HTMLButtonElement>(
    'button[aria-label="Dismiss waiting card"]',
  );
  assert(close, "visible close control has an accessible name");
  close.focus();
  assert(document.activeElement === close, "close control is keyboard focusable");
  await act(async () => {
    close.click();
  });
  assert(!host.querySelector('[data-agent-state="question"]'), "current waiting card is hidden");
  assert(cancelCalls === 0, "dismiss does not cancel");
  assert(stopCalls === 0, "dismiss does not stop");
  assert(terminalCalls === 0, "dismiss does not open or mutate the terminal");

  await act(async () => {
    root.render(
      <AgentStateIndicator
        state={{ state: "question" }}
        questionGeneration={1}
        onCancel={() => { cancelCalls += 1; }}
        onStop={() => { stopCalls += 1; }}
        onOpenTerminal={() => { terminalCalls += 1; }}
      />,
    );
  });
  assert(!host.querySelector('[data-agent-state="question"]'), "same generation stays dismissed");

  await act(async () => {
    root.render(
      <AgentStateIndicator
        state={{ state: "question" }}
        questionGeneration={2}
        onCancel={() => { cancelCalls += 1; }}
        onStop={() => { stopCalls += 1; }}
        onOpenTerminal={() => { terminalCalls += 1; }}
      />,
    );
  });
  assert(host.querySelector('[data-agent-state="question"]'), "new waiting generation resurfaces the card");
});

test("explicit question cancel keeps its existing handler", async () => {
  let cancelCalls = 0;
  const { host } = await render(
    <AgentStateIndicator
      state={{ state: "question" }}
      questionGeneration={1}
      onCancel={() => { cancelCalls += 1; }}
    />,
  );
  const cancel = host.querySelector<HTMLButtonElement>('[data-testid="agent-cancel-question"]');
  assert(cancel, "explicit Cancel control renders");
  await act(async () => {
    cancel.click();
  });
  assert(cancelCalls === 1, "explicit Cancel invokes its handler exactly once");
});

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
  const dots = host.querySelector(".activity-dots");
  assert(dots?.classList.contains("activity-dots--small"), "status row uses shared small dots");
  assert(dots?.classList.contains("activity-dots--accent"), "working dots use the accent tone");
  assert(dots?.querySelectorAll("i").length === 3, "working status keeps all three dots");
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
  const dots = host.querySelector(".activity-dots");
  assert(dots?.classList.contains("activity-dots--idle"), "summary settles the shared dots");
  assert(dots?.querySelectorAll("i").length === 3, "idle status retains the fixed three-dot slot");
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
