// A model reaching this component with `available: false` (e.g. gated by
// installed CLI version) must render disabled and unclickable — not vanish,
// and not be indistinguishable from a fully-selectable one.
// Run: npx tsx src/components/Terminal/ModelSuggestionPicker.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ModelSuggestionPicker } from "./ModelSuggestionPicker";
import type { ModelInfo } from "../../lib/types";

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
  for (const key of [
    "window", "document", "navigator", "HTMLElement", "HTMLButtonElement",
    "Element", "Node", "Event",
  ] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
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

setupDom();

const MODELS: ModelInfo[] = [
  { id: "sonnet", display_name: "Claude Sonnet" },
  { id: "fable", display_name: "Claude Fable", available: false },
];

test("a model gated unavailable renders disabled, marked, and unclickable", async () => {
  let picked: string | null = null;
  const { host, root } = await render(
    <ModelSuggestionPicker
      models={MODELS}
      highlighted={0}
      onHighlight={() => {}}
      onPick={(model) => { picked = model.id; }}
    />,
  );

  const buttons = Array.from(host.querySelectorAll("button")) as HTMLButtonElement[];
  assert(buttons.length === 2, "renders both models — the gated one is not silently omitted");

  const [sonnetBtn, fableBtn] = buttons;
  assert(!sonnetBtn.disabled, "an available model stays enabled");
  assert(fableBtn.disabled, "a gated-unavailable model renders disabled");
  assert(fableBtn.textContent!.includes("(unavailable)"), "a gated model is explicitly marked, not just visually dimmed");

  await act(async () => fableBtn.click());
  assert(picked === null, "clicking a disabled option never invokes onPick");

  await act(async () => sonnetBtn.click());
  assert((picked as string | null) === "sonnet", "an available option still invokes onPick normally");

  await act(async () => root.unmount());
});

async function main() {
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
  console.log(`ModelSuggestionPicker tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
