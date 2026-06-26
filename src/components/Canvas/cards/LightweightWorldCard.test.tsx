// Run: npx tsx src/components/Canvas/cards/LightweightWorldCard.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { LightweightWorldCard } from "./LightweightWorldCard";
import { FIXTURE_PLAN, FIXTURE_TASK } from "./fixtures.testdata";
import type { LaidOutNode } from "../layout";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const BOX = { x: 10, y: 20, width: 280, height: 160 };
const STYLE = { position: "absolute" as const, left: BOX.x, top: BOX.y, width: BOX.width, height: BOX.height };
const NODE: LaidOutNode = { id: "plan-alpha", kind: "plan", planId: "plan-alpha", box: BOX, iteration: null, manual: false };

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/lightweight-world-card" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

test("lightweight plan card exposes debug hooks and label metadata", async () => {
  const { host, root } = await render(
    <LightweightWorldCard kind="plan" node={NODE} model={FIXTURE_PLAN} style={STYLE} world="W1" />,
  );
  const card = host.querySelector('[data-testid="canvas-world-lightweight"]') as HTMLElement | null;
  assert(card, "lightweight root mounted");
  assert(card.getAttribute("data-lightweight-kind") === "plan", "kind hook");
  assert(card.getAttribute("data-world-render") === "lightweight", "render mode hook");
  assert(card.getAttribute("data-semantic-testid") === "canvas-plan-node", "semantic testid hook");
  assert(card.getAttribute("data-plan-id") === FIXTURE_PLAN.id, "plan id for focus");
  assert(card.querySelector('[data-testid="canvas-plan-coverage-bar"]'), "coverage accent");
  assert(!card.querySelector('[data-testid="canvas-plan-row"]'), "no rich section rows");
  await act(async () => root.unmount());
});

test("lightweight task card keeps status chip and identity hooks", async () => {
  const { host, root } = await render(
    <LightweightWorldCard
      kind="task"
      node={{ ...NODE, id: FIXTURE_TASK.id, kind: "task" }}
      model={{ task: FIXTURE_TASK }}
      style={STYLE}
      world="W1"
    />,
  );
  const card = host.querySelector('[data-testid="canvas-world-lightweight"][data-lightweight-kind="task"]') as HTMLElement | null;
  assert(card, "lightweight task mounted");
  assert(card.getAttribute("data-task-id") === FIXTURE_TASK.id, "task id for focus");
  assert(card.querySelector('[data-testid="canvas-task-name"]')?.textContent === "Dante", "task label");
  assert(card.querySelector('[data-testid="canvas-task-status"]'), "status chip");
  assert(!card.querySelector('[data-testid="canvas-task-full-body"]'), "no rich body");
  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} LightweightWorldCard tests passed`);
}

main();
