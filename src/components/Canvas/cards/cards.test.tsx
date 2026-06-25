// Run: npx tsx src/components/Canvas/cards/cards.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  MilestoneBubble,
  SectionNode,
  StrideSnake,
  TaskCard,
} from "./index";
import {
  FIXTURE_DRIFT,
  FIXTURE_MILESTONE,
  FIXTURE_NOT_MINE_DRIFT,
  FIXTURE_PLAN,
  FIXTURE_TAG_FACETS,
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

test("PLAN SectionNode renders title, section rows, output dots, and 50% coverage", async () => {
  const { host, root } = await render(
    <SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />,
  );
  assert(host.textContent?.includes("Auth rollout"), "plan title visible");
  assert(host.querySelector('[data-testid="canvas-plan-coverage-pct"]')?.textContent?.includes("50%"), "coverage pct");
  const rows = host.querySelectorAll('[data-testid="canvas-plan-row"]');
  assert(rows.length === 2, `two section rows (parent + subsection), got ${rows.length}`);
  assert(host.querySelectorAll('[data-testid="canvas-plan-output"]').length === 2, "an output dot per section row");
  await act(async () => root.unmount());
});

test("PLAN SectionNode: owner chip hidden for an all-empty OrchestratorRef (no 'unknown')", async () => {
  const empty = { ...FIXTURE_PLAN, owner: { pane_id: "", session_id: "", title: "" } };
  const { host, root } = await render(<SectionNode variant="plan" plan={empty} style={cardStyle} world="W3" />);
  assert(!host.querySelector('[data-testid="canvas-plan-owner"]'), "owner chip hidden when unowned");
  assert(!host.textContent?.toLowerCase().includes("unknown"), "no literal 'unknown' rendered");
  await act(async () => root.unmount());

  const sentinel = { ...FIXTURE_PLAN, owner: { pane_id: "unknown", session_id: null, title: "unknown" } };
  const s = await render(<SectionNode variant="plan" plan={sentinel} style={cardStyle} world="W3" />);
  assert(!s.host.querySelector('[data-testid="canvas-plan-owner"]'), "sentinel 'unknown' owner hides chip");
  await act(async () => s.root.unmount());

  const owned = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />);
  assert(owned.host.querySelector('[data-testid="canvas-plan-owner"]')?.textContent?.includes("Orchestrator"), "owner chip shows title when owned");
  await act(async () => owned.root.unmount());
});

test("PLAN SectionNode: uncovered section row pops with uncovered glyph", async () => {
  const { host, root } = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />);
  const oauth = host.querySelector('[data-section-id="sec-auth-oauth"]');
  assert(oauth?.getAttribute("data-row-glyph") === "uncovered", "uncovered subsection pops");
  const covered = host.querySelector('[data-section-id="sec-auth"]');
  assert(covered?.getAttribute("data-row-glyph") === "progress", "covered row is in-progress");
  await act(async () => root.unmount());
});

test("SPEC SectionNode: titles far (index), full body prose on zoom (content-LOD)", async () => {
  const far = await render(<SectionNode variant="spec" plan={FIXTURE_PLAN} style={{ ...cardStyle, height: 300 }} world="W1" />);
  assert(far.host.querySelector('[data-testid="canvas-spec-block"]'), "spec block present far");
  assert(!far.host.querySelector('[data-testid="canvas-spec-section-body"]'), "no prose far (index only)");
  await act(async () => far.root.unmount());

  const near = await render(<SectionNode variant="spec" plan={FIXTURE_PLAN} style={{ ...cardStyle, height: 300 }} world="W3" />);
  assert(near.host.querySelector('[data-testid="canvas-spec-section-body"]'), "prose revealed near");
  assert(near.host.textContent?.includes("sign in"), "section body markdown text visible near");
  await act(async () => near.root.unmount());
});

test("TaskCard freshness: compact relative stamp from status/updated time", async () => {
  const now = Date.now();
  const fresh = { ...FIXTURE_TASK, status_changed_at: now - 5 * 60_000 };
  const { host, root } = await render(<TaskCard task={fresh} style={cardStyle} world="W3" />);
  const el = host.querySelector('[data-testid="canvas-task-freshness"]');
  assert(el, "freshness counter rendered");
  assert(el.textContent?.includes("5m"), `expected 5m freshness, got ${el.textContent}`);
  assert((el.querySelector("span") as HTMLElement | null)?.title?.length, "absolute time title present");
  await act(async () => root.unmount());

  const far = await render(<TaskCard task={fresh} style={cardStyle} world="W0" />);
  const farEl = far.host.querySelector('[data-testid="canvas-task-freshness"]');
  assert(farEl?.textContent?.includes("5m"), `expected W0 freshness, got ${farEl?.textContent}`);
  await act(async () => far.root.unmount());
});

test("TaskCard freshness: absent when no timestamp fields", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={cardStyle} world="W3" />);
  assert(!host.querySelector('[data-testid="canvas-task-freshness"]'), "no freshness without stamps");
  await act(async () => root.unmount());
});

test("TaskCard (near/full): name, model chip, status, and markdown body", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} style={cardStyle} world="W3" />);
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
    <TaskCard task={assigned} style={cardStyle} world="W3" onOpenPane={(id) => (opened = id)} />,
  );
  const chip = host.querySelector('[data-testid="canvas-task-pane-link"]') as HTMLButtonElement;
  assert(chip, "pane link chip rendered");
  await act(async () => chip.click());
  assert(opened === "pane-42", `onOpenPane fired with pane id, got ${opened}`);
  await act(async () => root.unmount());
});

test("TaskCard drift badge renders actionable, muted not_mine, and absent states", async () => {
  const plain = await render(<TaskCard task={FIXTURE_TASK} style={cardStyle} world="W3" />);
  assert(!plain.host.querySelector('[data-testid="canvas-task-drift-badge"]'), "no badge without drift");
  await act(async () => plain.root.unmount());

  const active = await render(<TaskCard task={FIXTURE_TASK} drift={FIXTURE_DRIFT} style={cardStyle} world="W3" />);
  const activeBadge = active.host.querySelector('[data-testid="canvas-task-drift-badge"]') as HTMLElement | null;
  assert(activeBadge, "drift badge rendered");
  assert(activeBadge.getAttribute("data-drift-kind") === "in_progress_pane_gone", "actionable kind attached");
  assert(activeBadge.getAttribute("data-not-mine") === "false", "actionable drift is not muted");
  assert(activeBadge.getAttribute("title")?.includes(FIXTURE_DRIFT.suggested_fix), "tooltip includes suggested fix");
  await act(async () => active.root.unmount());

  const muted = await render(<TaskCard task={FIXTURE_TASK} drift={FIXTURE_NOT_MINE_DRIFT} style={cardStyle} world="W3" />);
  const mutedBadge = muted.host.querySelector('[data-testid="canvas-task-drift-badge"]') as HTMLElement | null;
  assert(mutedBadge, "not_mine drift badge rendered");
  assert(mutedBadge.getAttribute("data-not-mine") === "true", "not_mine drift marked muted");
  assert((mutedBadge.getAttribute("style") ?? "").includes("opacity: 0.55"), "not_mine drift uses muted opacity");
  await act(async () => muted.root.unmount());
});

test("TaskCard content-LOD: compact far (name+status, no body) → full near (body)", async () => {
  const far = await render(<TaskCard task={FIXTURE_TASK} tagFacets={[...FIXTURE_TAG_FACETS]} style={cardStyle} world="W0" />);
  assert(far.host.querySelector('[data-testid="canvas-task-name"]'), "name present far");
  assert(far.host.querySelector('[data-testid="canvas-task-status"]'), "status present far");
  assert(!far.host.querySelector('[data-testid="canvas-task-body"]'), "no full body far (compact)");
  assert(far.host.querySelector('[data-testid="canvas-task-tile-tags"]'), "tile tag payload far");
  assert(!far.host.querySelector('[data-testid="canvas-task-digest"]'), "digest hidden when tags substitute body");
  assert(far.host.querySelector('[data-testid="canvas-task-primary-facet"]')?.textContent?.includes("Navigation"), "primary facet chip far");
  await act(async () => far.root.unmount());

  const near = await render(<TaskCard task={FIXTURE_TASK} tagFacets={[...FIXTURE_TAG_FACETS]} style={cardStyle} world="W3" />);
  assert(near.host.querySelector('[data-testid="canvas-task-body"]'), "body present near (full)");
  assert(near.host.querySelectorAll('[data-testid="canvas-task-tag-facet"]').length >= 1, "facet chips in meta band near");
  await act(async () => near.root.unmount());
});

test("TaskCard primary facet tints left border accent", async () => {
  const { host, root } = await render(<TaskCard task={FIXTURE_TASK} tagFacets={[...FIXTURE_TAG_FACETS]} style={cardStyle} world="W1" />);
  const card = host.querySelector('[data-testid="canvas-task-card"]') as HTMLElement | null;
  assert(card?.style.borderLeftColor === "rgba(99, 102, 241, 0.95)", `primary facet border accent, got: ${card?.getAttribute("style")}`);
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

test("StrideSnake W3 bloom renders attached discussion doc collapsed by default", async () => {
  const milestone = {
    ...FIXTURE_MILESTONE,
    text: "STATUS: done\n\ndoc: notes/design.md\n\n# Design notes\n\nExpand me.",
  };
  const { host, root } = await render(
    <StrideSnake milestones={[milestone]} world="W3" width={280} />,
  );
  assert(host.querySelector('[data-testid="attached-discussion-doc"]'), "discussion doc chrome");
  assert(host.textContent?.includes("STATUS: done"), "summary visible");
  assert(!host.textContent?.includes("Expand me."), "doc body hidden until expand");
  assert(host.querySelector('[data-testid="attached-discussion-toggle"]'), "expand toggle");
  await act(async () => root.unmount());
});

test("§34 crisp text: readable band wraps card in a counter-scaled re-raster layer", async () => {
  // crispScale 1 (default / birds-eye) → single layer, no counter-scale.
  const plain = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" />);
  assert(!plain.host.querySelector(".ct-canvas-crisp"), "no crisp layer at 1×");
  assert(plain.host.querySelector('[data-testid="canvas-plan-node"]'), "card still renders at 1×");
  await act(async () => plain.root.unmount());

  // crispScale 4 (deep zoom) → zoom 4 + transform scale(0.25) re-rasterize layer.
  const deep = await render(<SectionNode variant="plan" plan={FIXTURE_PLAN} style={cardStyle} world="W3" crispScale={4} />);
  const layer = deep.host.querySelector(".ct-canvas-crisp") as HTMLElement | null;
  assert(layer, "crisp layer present at 4×");
  const css = layer!.getAttribute("style") ?? "";
  assert(css.includes("scale(0.25)"), `counter-scale 1/4 applied, got: ${css}`);
  assert(css.replace(/\s/g, "").includes("zoom:4"), `zoom 4 applied, got: ${css}`);
  // Content still reaches the DOM through the wrapper (title visible, crisp).
  assert(deep.host.textContent?.includes("Auth rollout"), "title still rendered inside crisp layer");
  await act(async () => deep.root.unmount());
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
