// Run: npx tsx src/components/Canvas/cards/cards.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  MilestoneBubble,
  PlanNode,
  SpecNode,
  TaskCard,
} from "./index";
import {
  FIXTURE_MILESTONE,
  FIXTURE_PLAN,
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

const cardStyle = { position: "absolute" as const, left: 0, top: 0, width: 300 };

test("PlanNode renders title, section rows, and 50% coverage", async () => {
  const { host, root } = await render(
    <PlanNode plan={FIXTURE_PLAN} style={cardStyle} lod="near" />,
  );
  assert(host.textContent?.includes("Auth rollout"), "plan title visible");
  assert(host.querySelector('[data-testid="canvas-plan-coverage-pct"]')?.textContent?.includes("50%"), "coverage pct");
  const rows = host.querySelectorAll('[data-testid="canvas-plan-row"]');
  assert(rows.length === 2, `two section rows (parent + subsection), got ${rows.length}`);
  await act(async () => root.unmount());
});

test("PlanNode: owner chip hidden for an all-empty OrchestratorRef (no 'unknown')", async () => {
  const empty = {
    ...FIXTURE_PLAN,
    owner: { pane_id: "", session_id: "", title: "" },
  };
  const { host, root } = await render(<PlanNode plan={empty} style={cardStyle} lod="near" />);
  assert(!host.querySelector('[data-testid="canvas-plan-owner"]'), "owner chip hidden when unowned");
  assert(!host.textContent?.toLowerCase().includes("unknown"), "no literal 'unknown' rendered");
  await act(async () => root.unmount());

  // Backend placeholder "unknown" must also read as unowned, not literal text.
  const sentinel = { ...FIXTURE_PLAN, owner: { pane_id: "unknown", session_id: null, title: "unknown" } };
  const s = await render(<PlanNode plan={sentinel} style={cardStyle} lod="near" />);
  assert(!s.host.querySelector('[data-testid="canvas-plan-owner"]'), "sentinel 'unknown' owner hides chip");
  assert(!s.host.textContent?.toLowerCase().includes("unknown"), "no literal 'unknown' rendered for sentinel");
  await act(async () => s.root.unmount());

  const owned = await render(<PlanNode plan={FIXTURE_PLAN} style={cardStyle} lod="near" />);
  assert(owned.host.querySelector('[data-testid="canvas-plan-owner"]')?.textContent?.includes("Orchestrator"), "owner chip shows title when owned");
  await act(async () => owned.root.unmount());
});

test("PlanNode: uncovered section row pops with uncovered glyph", async () => {
  const { host, root } = await render(
    <PlanNode plan={FIXTURE_PLAN} style={cardStyle} lod="near" />,
  );
  const oauth = host.querySelector('[data-section-id="sec-auth-oauth"]');
  assert(oauth?.getAttribute("data-row-glyph") === "uncovered", "uncovered subsection pops");
  const covered = host.querySelector('[data-section-id="sec-auth"]');
  assert(covered?.getAttribute("data-row-glyph") === "progress", "covered row is in-progress");
  await act(async () => root.unmount());
});

test("SpecNode renders headings and markdown body", async () => {
  const { host, root } = await render(
    <SpecNode plan={FIXTURE_PLAN} style={cardStyle} lod="near" />,
  );
  const headings = host.querySelectorAll('[data-testid="canvas-spec-heading"]');
  assert(headings.length >= 2, `spec renders section + subsection headings, got ${headings.length}`);
  assert(host.querySelector('[data-testid="canvas-spec-body"]'), "spec body mounted");
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

test("TaskCard pane link chip appears for assigned tasks and fires onOpenPane", async () => {
  let opened: string | null = null;
  const assigned = { ...FIXTURE_TASK, assigned_pane_id: "pane-42" };
  const { host, root } = await render(
    <TaskCard task={assigned} style={cardStyle} lod="near" onOpenPane={(id) => (opened = id)} />,
  );
  const chip = host.querySelector('[data-testid="canvas-task-pane-link"]') as HTMLButtonElement;
  assert(chip, "pane link chip rendered");
  await act(async () => chip.click());
  assert(opened === "pane-42", `onOpenPane fired with pane id, got ${opened}`);
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

test("MilestoneBubble shows kind initial + iteration at rest; tooltip is hover-only (no near-LOD wall)", async () => {
  const { host, root } = await render(
    <MilestoneBubble model={{ milestone: FIXTURE_MILESTONE, color: "rgba(99,102,241,0.9)" }} style={cardStyle} lod="near" />,
  );
  assert(host.querySelector('[data-testid="canvas-milestone-initial"]')?.textContent === "S", "kind initial glyph at rest");
  assert(host.querySelector('[data-testid="canvas-milestone-iteration"]')?.textContent === "2", "iteration badge");
  // The bug: at near LOD every bubble auto-opened its tooltip → overlapping,
  // clipped, word-glued mess. Tooltip must NOT render until hover.
  assert(!host.querySelector('[data-testid="canvas-milestone-text"]'), "no auto tooltip at near LOD");
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
