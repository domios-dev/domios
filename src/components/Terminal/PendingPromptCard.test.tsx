// Run: npx tsx src/components/Terminal/PendingPromptCard.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PendingPromptCard } from "./PendingPromptCard";
import type { PendingPrompt } from "../../lib/types";

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
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function card(host: HTMLElement): HTMLElement | null {
  return host.querySelector('[data-testid="pane-pending-prompt"]');
}

function optionButtons(host: HTMLElement): HTMLButtonElement[] {
  return Array.from(host.querySelectorAll('[data-testid="pane-pending-prompt-option"]'));
}

const MODEL_PROMPT: PendingPrompt = {
  question: "Select a model",
  options: [
    { label: "Opus", value: "1" },
    { label: "Sonnet", value: "2" },
    { label: "Haiku", value: "3" },
  ],
};

const WIZARD_PROMPT: PendingPrompt = {
  question: "Which environment should this setup target?",
  sections: [
    { label: "Environment", active: true },
    { label: "Autonomy" },
    { label: "Scope" },
    { label: "Verbosity" },
  ],
  options: [
    { label: "Dev", value: "1" },
    { label: "Prod", value: "2" },
  ],
  multi_select: false,
  submit_available: true,
};

const MULTI_PROMPT: PendingPrompt = {
  question: "Which focus areas should be enabled?",
  sections: [
    { label: "Environment", completed: true },
    { label: "Focus", active: true },
  ],
  options: [
    { label: "Performance", value: "1" },
    { label: "Tests", value: "2" },
    { label: "Docs", value: "3" },
  ],
  multi_select: true,
  submit_available: true,
  selected_values: ["2"],
};

setupDom();

test("renders nothing when there is no pending prompt", async () => {
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={null} onRespond={() => {}} />,
  );
  assert(!card(host), "no card should render without a pending prompt");
  await act(async () => root.unmount());
});

test("renders the question and one button per option", async () => {
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={MODEL_PROMPT} onRespond={() => {}} />,
  );
  assert(card(host), "card should render when a prompt is present");
  const q = host.querySelector('[data-testid="pane-pending-prompt-question"]');
  assert(q?.textContent === "Select a model", `question should render, got: ${q?.textContent}`);
  const opts = optionButtons(host);
  assert(opts.length === MODEL_PROMPT.options.length, `expected ${MODEL_PROMPT.options.length} option buttons, got ${opts.length}`);
  assert(opts[0].textContent?.includes("Opus"), `first option label should include Opus, got: ${opts[0].textContent}`);
  assert(opts[2].textContent?.includes("Haiku"), `last option label should include Haiku, got: ${opts[2].textContent}`);
  await act(async () => root.unmount());
});

test("card has no cancel/dismiss affordance", async () => {
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={MODEL_PROMPT} onRespond={() => {}} />,
  );
  const cancel = host.querySelector('[data-testid="pane-pending-prompt-cancel"]');
  assert(!cancel, "non-dismissable card must not render a cancel button");
  await act(async () => root.unmount());
});

test("clicking option i calls onRespond with that option's value", async () => {
  const calls: string[] = [];
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={MODEL_PROMPT} onRespond={(v) => calls.push(v)} />,
  );
  await act(async () => {
    optionButtons(host)[1].click();
  });
  assert(calls.length === 1, `onRespond should fire once, got ${calls.length}`);
  assert(calls[0] === "2", `onRespond should receive options[1].value ("2"), got: ${calls[0]}`);
  await act(async () => root.unmount());
});

test("wizard prompt renders section progress and submit", async () => {
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={WIZARD_PROMPT} onRespond={() => {}} />,
  );
  const sections = host.querySelector('[data-testid="pane-pending-prompt-sections"]');
  assert(sections?.textContent?.includes("Environment"), "wizard sections should render");
  const submit = host.querySelector('[data-testid="pane-pending-prompt-submit"]') as HTMLButtonElement | null;
  assert(submit, "wizard submit should render");
  await act(async () => root.unmount());
});

test("wizard single-select option preserves option value", async () => {
  const calls: string[] = [];
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={WIZARD_PROMPT} onRespond={(v) => calls.push(v)} />,
  );
  await act(async () => {
    optionButtons(host)[1].click();
  });
  assert(calls[0] === "2", `wizard option should send value 2, got ${calls[0]}`);
  await act(async () => root.unmount());
});

test("wizard multi-select confirm sends selected bitmask sentinel", async () => {
  const calls: string[] = [];
  const { host, root } = await render(
    <PendingPromptCard pendingPrompt={MULTI_PROMPT} onRespond={(v) => calls.push(v)} />,
  );
  await act(async () => {
    optionButtons(host)[0].click();
    optionButtons(host)[2].click();
  });
  const confirm = host.querySelector('[data-testid="pane-pending-prompt-confirm"]') as HTMLButtonElement | null;
  assert(confirm, "multi-select confirm should render");
  await act(async () => {
    confirm.click();
  });
  assert(calls[0] === "10007", `selected values 1,2,3 should send mask value 10007, got ${calls[0]}`);
  await act(async () => root.unmount());
});

async function main() {
  const started = performance.now();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  const elapsedMs = performance.now() - started;
  console.log(`PendingPromptCard tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
