// Run: npx tsx src/components/Canvas/cards/viewModes.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PlanNode, SpecSectionCard, TaskCard } from "./index";
import { FIXTURE_MILESTONE, FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "./fixtures.testdata";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/canvas-view-modes" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLPreElement", "Node", "Event", "CustomEvent"] as const) {
    Object.defineProperty(globalThis, k, { value: (window as unknown as Record<string, unknown>)[k] ?? (window as { document: unknown }).document, configurable: true });
  }
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  await act(async () => { await Promise.resolve(); });
  return { host, root };
}

const style = { position: "absolute" as const, left: 0, top: 0, width: 300 };
const q = (h: HTMLElement, sel: string) => h.querySelector(sel);
const qa = (h: HTMLElement, sel: string) => h.querySelectorAll(sel);

// TASK ------------------------------------------------------------------------
test("TaskCard document mode mounts the full markdown body", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={style} viewMode="document" />);
  assert(q(host, '[data-view-mode="document"]'), "document frame");
  assert(q(host, '[data-testid="canvas-task-body"]'), "full body mounted");
  assert(!q(host, '[data-testid="canvas-task-digest"]'), "no digest in document mode");
  await act(async () => root.unmount());
});

test("TaskCard card mode shows a digest, NOT the full markdown body (bounded)", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={style} viewMode="card" />);
  assert(q(host, '[data-view-mode="card"]'), "card frame");
  assert(q(host, '[data-testid="canvas-task-name"]')?.textContent === "Dante", "name present");
  assert(q(host, '[data-testid="canvas-task-status"]'), "status badge present");
  assert(q(host, '[data-testid="canvas-task-digest"]'), "digest present");
  assert(!q(host, '[data-testid="canvas-task-body"]'), "full markdown body NOT mounted in card mode");
  await act(async () => root.unmount());
});

test("TaskCard tile mode shows only name + glyph (no body, no digest) — clean far view", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={style} viewMode="tile" />);
  assert(q(host, '[data-view-mode="tile"]'), "tile frame");
  assert(q(host, '[data-testid="canvas-task-name"]')?.textContent === "Dante", "name present");
  assert(q(host, '[data-testid="canvas-task-marker"]'), "status glyph present");
  assert(!q(host, '[data-testid="canvas-task-digest"]'), "no digest in tile mode");
  assert(!q(host, '[data-testid="canvas-task-body"]'), "no body in tile mode");
  await act(async () => root.unmount());
});

// STEPS (Thread & Coil interior) -----------------------------------------------
const taskWithSteps = { ...FIXTURE_TASK, milestones: [FIXTURE_MILESTONE, { ...FIXTURE_MILESTONE, id: "ms-2", kind: "testing", text: "ran the suite, all green" }] };

test("Document task shows step CARDS with the milestone TEXT (the dots-only bug fix)", async () => {
  const { host, root } = await render(<TaskCard task={taskWithSteps} style={style} viewMode="document" />);
  assert(q(host, '[data-step-mode="document"]'), "document step mode");
  assert(qa(host, '[data-testid="canvas-task-step-card"]').length === 2, "a step card per milestone");
  assert(host.textContent?.includes("ran the suite"), "milestone text is visible (not just a dot)");
  await act(async () => root.unmount());
});

test("Tile/Card tasks show step DOTS (constellation/thread), not text cards", async () => {
  const tile = await render(<TaskCard task={taskWithSteps} style={style} viewMode="tile" />);
  assert(q(tile.host, '[data-step-mode="tile"]'), "tile step mode");
  assert(qa(tile.host, '[data-testid="canvas-task-step-dot"]').length === 2, "tile shows step dots");
  assert(!q(tile.host, '[data-testid="canvas-task-step-card"]'), "no text cards in tile");
  await act(async () => tile.root.unmount());

  const card = await render(<TaskCard task={taskWithSteps} style={style} viewMode="card" />);
  assert(q(card.host, '[data-step-mode="card"]'), "card step mode");
  assert(!q(card.host, '[data-testid="canvas-task-step-card"]'), "no text cards in card mode");
  await act(async () => card.root.unmount());
});

// SPEC ------------------------------------------------------------------------
test("SpecSectionCard: document=body, card=digest, tile=title-only", async () => {
  const doc = await render(<SpecSectionCard section={FIXTURE_SECTION} glyph="progress" style={style} viewMode="document" />);
  assert(q(doc.host, '[data-testid="canvas-spec-section-body"]'), "document body mounted");
  assert(!q(doc.host, '[data-testid="canvas-spec-section-digest"]'), "no digest in document");
  await act(async () => doc.root.unmount());

  const card = await render(<SpecSectionCard section={FIXTURE_SECTION} glyph="progress" style={style} viewMode="card" />);
  assert(q(card.host, '[data-testid="canvas-spec-section-digest"]'), "card digest mounted");
  assert(!q(card.host, '[data-testid="canvas-spec-section-body"]'), "no full body in card");
  await act(async () => card.root.unmount());

  const tile = await render(<SpecSectionCard section={FIXTURE_SECTION} glyph="progress" style={style} viewMode="tile" />);
  assert(q(tile.host, '[data-testid="canvas-spec-section-title"]')?.textContent === "Authentication", "tile title present");
  assert(!q(tile.host, '[data-testid="canvas-spec-section-digest"]'), "no digest in tile");
  assert(!q(tile.host, '[data-testid="canvas-spec-section-body"]'), "no body in tile");
  await act(async () => tile.root.unmount());
});

// PLAN ------------------------------------------------------------------------
test("PlanNode: rows only in document mode; card/tile keep title + coverage (no empty box)", async () => {
  const doc = await render(<PlanNode plan={FIXTURE_PLAN} style={style} viewMode="document" />);
  assert(doc.host.querySelectorAll('[data-testid="canvas-plan-row"]').length === 2, "document shows section rows");
  await act(async () => doc.root.unmount());

  const card = await render(<PlanNode plan={FIXTURE_PLAN} style={style} viewMode="card" />);
  assert(!card.host.querySelector('[data-testid="canvas-plan-row"]'), "card has no rows");
  assert(card.host.textContent?.includes("Auth rollout"), "card keeps title");
  assert(card.host.querySelector('[data-testid="canvas-plan-coverage-pct"]'), "card keeps coverage");
  await act(async () => card.root.unmount());

  const tile = await render(<PlanNode plan={FIXTURE_PLAN} style={style} viewMode="tile" />);
  assert(!tile.host.querySelector('[data-testid="canvas-plan-row"]'), "tile has no rows");
  assert(tile.host.textContent?.includes("Auth rollout"), "tile keeps title");
  assert(tile.host.querySelector('[data-testid="canvas-plan-coverage-pct"]'), "tile keeps coverage glyph");
  await act(async () => tile.root.unmount());
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
      console.error(`  ✗ ${name}\n    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
