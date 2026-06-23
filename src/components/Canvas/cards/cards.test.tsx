// Run: npx tsx src/components/Canvas/cards/cards.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  MilestoneNode,
  PlanFrame,
  SectionCard,
  TaskCard,
} from "./index";
import {
  FIXTURE_MILESTONE,
  FIXTURE_PLAN,
  FIXTURE_SECTION,
  FIXTURE_TASK,
} from "./fixtures.testdata";

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
  const window = new Window({ url: "http://localhost/canvas-cards" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLPreElement", { value: window.HTMLPreElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "CustomEvent", { value: window.CustomEvent, configurable: true });
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

const cardStyle = { position: "absolute" as const, left: 0, top: 0, width: 240 };

test("PlanFrame renders title and coverage meter", async () => {
  const { host, root } = await render(
    <PlanFrame plan={FIXTURE_PLAN} style={cardStyle} lod="near" />,
  );
  assert(host.textContent?.includes("Auth rollout"), "plan title visible");
  assert(host.querySelector('[data-testid="canvas-plan-coverage-pct"]')?.textContent === "50%", "coverage pct");
  await act(async () => root.unmount());
});

test("SectionCard renders title, subsections indicator, and markdown body", async () => {
  const { host, root } = await render(
    <SectionCard section={FIXTURE_SECTION} style={cardStyle} lod="near" />,
  );
  assert(host.textContent?.includes("Authentication"), "section title");
  assert(host.querySelector('[data-testid="canvas-section-subsections"]')?.textContent?.includes("1 subsection"), "subsection count");
  assert(host.querySelector('[data-testid="canvas-section-body"]'), "markdown body mounted");
  await act(async () => root.unmount());
});

test("TaskCard renders name, model chip, status, and markdown body", async () => {
  const { host, root } = await render(
    <TaskCard task={FIXTURE_TASK} style={cardStyle} lod="near" />,
  );
  assert(host.querySelector('[data-testid="canvas-task-name"]')?.textContent === "Dante", "task name");
  assert(host.querySelector('[data-testid="canvas-task-model"]')?.textContent === "claude:opus", "model chip");
  assert(host.querySelector('[data-testid="canvas-task-status"]')?.textContent === "90% done", "status label");
  assert(host.querySelector('[data-testid="canvas-task-body"]'), "task markdown body");
  await act(async () => root.unmount());
});

test("TaskCard far LOD hides model and body", async () => {
  const { host, root } = await render(
    <TaskCard task={FIXTURE_TASK} style={cardStyle} lod="far" />,
  );
  assert(host.querySelector('[data-testid="canvas-task-name"]'), "name still visible");
  assert(!host.querySelector('[data-testid="canvas-task-model"]'), "model hidden at far LOD");
  assert(!host.querySelector('[data-testid="canvas-task-body"]'), "body hidden at far LOD");
  await act(async () => root.unmount());
});

test("MilestoneNode renders kind, markdown text, and iteration badge", async () => {
  const { host, root } = await render(
    <MilestoneNode milestone={FIXTURE_MILESTONE} style={cardStyle} lod="near" />,
  );
  assert(host.querySelector('[data-testid="canvas-milestone-kind"]')?.textContent?.includes("status update"), "kind label");
  assert(host.querySelector('[data-testid="canvas-milestone-text"]'), "milestone markdown");
  assert(host.querySelector('[data-testid="canvas-milestone-iteration"]')?.textContent === "2", "iteration badge");
  await act(async () => root.unmount());
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
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
