// Run: npx tsx src/components/Canvas/cards/sectionedPlanNode.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { SectionedPlanNode } from "./index";
import { FIXTURE_PLAN } from "./fixtures.testdata";
import type { CanvasViewMode } from "./types";

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
  const window = new Window({ url: "http://localhost/sectioned-plan" });
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
  await act(async () => root.render(ui));
  await act(async () => { await Promise.resolve(); });
  return { host, root };
}

const style = (mode: CanvasViewMode) => ({ position: "absolute" as const, left: 0, top: 0, width: 480, height: mode === "tile" ? 280 : mode === "card" ? 400 : 640 });
const q = (h: HTMLElement, s: string) => h.querySelector(s);
const qa = (h: HTMLElement, s: string) => h.querySelectorAll(s);

test("rail is ALWAYS rendered with one output dot per section (the task anchors)", async () => {
  for (const mode of ["document", "card", "tile"] as CanvasViewMode[]) {
    const { host, root } = await render(<SectionedPlanNode plan={FIXTURE_PLAN} style={style(mode)} viewMode={mode} />);
    assert(q(host, '[data-testid="canvas-plan-rail"]'), `${mode}: rail present`);
    // FIXTURE_PLAN has 2 flattened sections (Authentication + OAuth flow).
    assert(qa(host, '[data-testid="canvas-plan-output"]').length === 2, `${mode}: one output dot per section`);
    await act(async () => root.unmount());
  }
});

test("Document mode = full section bodies (RichMarkdown) with internal-scroll body; rail rows are the TOC", async () => {
  const { host, root } = await render(<SectionedPlanNode plan={FIXTURE_PLAN} style={style("document")} viewMode="document" />);
  assert(q(host, '[data-view-mode="document"]'), "document frame");
  const body = q(host, '[data-testid="canvas-plan-body"]') as HTMLElement;
  assert(body && (body.style.overflowY === "auto"), "body internal-scrolls inside the fixed frame");
  assert(qa(host, '[data-testid="canvas-plan-doc-section"]').length === 2, "a body block per section");
  assert(host.textContent?.includes("sign in"), "section markdown body text rendered (spec prose = section bodies)");
  assert(qa(host, '[data-testid="canvas-plan-rail-row"]').length === 2, "rail rows present as the in-node TOC");
  await act(async () => root.unmount());
});

test("Card mode = section titles + digests (no full body); Tile mode = coverage summary (no sections)", async () => {
  const card = await render(<SectionedPlanNode plan={FIXTURE_PLAN} style={style("card")} viewMode="card" />);
  assert(qa(card.host, '[data-testid="canvas-plan-card-row"]').length === 2, "card rows per section");
  assert(!q(card.host, '[data-testid="canvas-plan-doc-section"]'), "no full doc bodies in card mode");
  await act(async () => card.root.unmount());

  const tile = await render(<SectionedPlanNode plan={FIXTURE_PLAN} style={style("tile")} viewMode="tile" />);
  // Tile body is a compact SECTION INDEX (not a giant coverage panel).
  assert(qa(tile.host, '[data-testid="canvas-plan-tile-row"]').length === 2, "tile shows a section-row index");
  assert(!q(tile.host, '[data-testid="canvas-plan-card-row"]'), "no card rows in tile mode");
  assert(!q(tile.host, '[data-testid="canvas-plan-doc-section"]'), "no full doc bodies in tile mode");
  assert(q(tile.host, '[data-testid="canvas-plan-coverage-pct"]'), "tile keeps the compact coverage bar (header)");
  await act(async () => tile.root.unmount());
});

test("§23.7 content-LOD: zooming in (contentNear) reveals section BODIES inside the SAME fixed frame, any mode", async () => {
  // Tile frame, far: section index, no bodies.
  const far = await render(<SectionedPlanNode plan={FIXTURE_PLAN} style={style("tile")} viewMode="tile" contentNear={false} />);
  assert(q(far.host, '[data-testid="canvas-plan-body"]')?.getAttribute("data-content") === "index", "far tile = index");
  assert(!q(far.host, '[data-testid="canvas-plan-doc-section"]'), "far tile shows no bodies");
  await act(async () => far.root.unmount());

  // Tile frame, NEAR: bodies fade in inside the SAME (still-Tile-sized) frame.
  const near = await render(<SectionedPlanNode plan={FIXTURE_PLAN} style={style("tile")} viewMode="tile" contentNear={true} />);
  assert(q(near.host, '[data-testid="canvas-plan-body"]')?.getAttribute("data-content") === "body", "near tile reveals bodies");
  assert(q(near.host, '[data-testid="canvas-plan-doc-section"]'), "near tile mounts section bodies");
  assert(near.host.textContent?.includes("sign in"), "near tile shows the spec prose");
  assert(q(near.host, "[data-canvas-scroll-body]"), "near body is internally scrollable (§23.6)");
  await act(async () => near.root.unmount());
});

test("header mode switch + fit button fire their callbacks (manual control, §23.1)", async () => {
  let picked: CanvasViewMode | null = null;
  let fitted = false;
  const { host, root } = await render(
    <SectionedPlanNode plan={FIXTURE_PLAN} style={style("document")} viewMode="document" onSetMode={(m) => (picked = m)} onFitNode={() => (fitted = true)} />,
  );
  const tileBtn = q(host, '[data-testid="canvas-plan-mode-tile"]') as HTMLButtonElement;
  assert(tileBtn, "tile mode button present");
  await act(async () => tileBtn.click());
  assert(picked === "tile", "clicking the mode button selects that mode");
  const fitBtn = q(host, '[data-testid="canvas-plan-fit"]') as HTMLButtonElement;
  await act(async () => fitBtn.click());
  assert(fitted, "fit button fires onFitNode");
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
      console.error(`  ✗ ${name}\n    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
