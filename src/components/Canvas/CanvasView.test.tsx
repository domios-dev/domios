import { muxMockFetch } from "../../lib/muxMockFetch.testdata";
import { throws } from "node:assert/strict";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import type { Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { bindFixtureOwnersToWorkspace, CanvasViewInner, fixtureOwnerTabIds } from "./CanvasView";
import { filterPlansForCanvas } from "./planTabMatch";
import type { RenderReadbackNode } from "./canvasRender/drawScene";
import type { Plan } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
const frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
let frameTime = 0;
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const STABLE_TABS = {};
// FIXTURE_PLAN owner is orch-1 → bind it to the current workspace for scoping.
const WS_ID = "ws-test";
const WORKSPACES: Workspace[] = [
  { id: WS_ID, name: "Test", tabs: ["orch-1"], active_tab_id: null, cwd: null },
];
const CANVAS_PROPS = {
  mode: "workspace" as const,
  tabs: STABLE_TABS,
  viewportPadding: 10000,
  workspaceId: WS_ID,
  workspaces: WORKSPACES,
};

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/canvas-view" });
  Object.assign(window, { __CODETERM_DAEMON__: { origin: /^https?:/.test(window.location.origin) ? window.location.origin : "http://127.0.0.1:7686", token: "test-token" } });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: class {
      observe() {}
      disconnect() {}
    },
  });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  for (const target of [globalThis, window]) {
    Object.defineProperty(target, "requestAnimationFrame", { configurable: true, value: (callback: FrameRequestCallback) => {
      const id = ++nextFrame;
      frames.set(id, callback);
      return id;
    } });
    Object.defineProperty(target, "cancelAnimationFrame", { configurable: true, value: (id: number) => frames.delete(id) });
  }
  __setListenForTest(async () => () => {});
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root; rerender: (ui: ReactElement) => Promise<void> }> {
  const host = document.createElement("div");
  host.style.width = "1200px";
  host.style.height = "800px";
  document.body.appendChild(host);
  const root = createRoot(host);
  const rerender = async (next: ReactElement) => {
    await act(async () => {
      root.render(next);
    });
    await act(async () => {
      await Promise.resolve();
    });
  };
  await rerender(ui);
  return { host, root, rerender };
}

function readback(): RenderReadbackNode[] {
  return (window as unknown as { __codetermCanvasRender?: { nodes: RenderReadbackNode[] } }).__codetermCanvasRender?.nodes ?? [];
}
function nodeOf(kind: string, id?: string): RenderReadbackNode | undefined {
  return readback().find((n) => n.kind === kind && (id ? n.id === id : true));
}
async function paintFrames() {
  for (let batch = 0; frames.size && batch < 32; batch++) {
    await act(async () => {
      frameTime += 1000 / 60;
      for (const [id, callback] of [...frames]) {
        if (!frames.delete(id)) continue;
        callback(frameTime);
      }
    });
  }
  assert(frames.size === 0, "canvas frame work completes within 32 controlled frames");
}
function dblclickAt(host: HTMLElement, x: number, y: number) {
  const canvas = host.querySelector('[data-testid="canvas-world-canvas"]') as HTMLElement;
  canvas.dispatchEvent(new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("dblclick", { bubbles: true, clientX: x, clientY: y }));
}

const FIXTURE_PLAN_WITH_SPEC: Plan = {
  ...FIXTURE_PLAN,
  primary_spec_id: "spec-fixture",
};

test("canvas renders SEPARATE spec + plan + task nodes with their card content", async () => {
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN_WITH_SPEC]} />);
  await paintFrames();
  assert(host.querySelector('[data-testid="canvas-view"]'), "canvas shell");
  assert(host.querySelector('[data-testid="canvas-world-canvas"]'), "world <canvas> mounted");

  const plan = nodeOf("plan");
  assert(plan && plan.kicker === "PLAN" && plan.title === "Auth rollout", "plan node drawn with title");
  assert(plan!.sectionTitles.includes("Authentication"), "plan card lists its section titles");

  const spec = nodeOf("spec");
  assert(spec && spec.kicker === "SPEC", "separate spec node drawn");
  assert(spec!.sectionTitles.length > 0, "spec card lists section titles");

  const task = nodeOf("task", FIXTURE_TASK.id);
  assert(task && task.kicker === "TASK" && task.title === "Dante", "task node drawn with name");
  assert(task!.fields.some((f) => f.startsWith("status:")), "task card carries status field");
  await act(async () => root.unmount());
});

test("double-click focuses a node — no breadcrumb UI", async () => {
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN]} />);
  await paintFrames();
  const box = window.__codetermCanvasScene?.get("workspace:workspace")?.nodeScreenBox(FIXTURE_TASK.id);
  assert(box, "scene readback exposes the task screen box");
  await act(async () => {
    dblclickAt(host, box!.x + box!.width / 2, box!.y + box!.height / 2);
  });
  await paintFrames();
  // breadcrumb bar is GONE for good — focusing/Esc-return work without that top UI.
  assert(host.querySelector('[data-testid="canvas-world-canvas"]'), "world canvas still mounted after focus");
  assert(window.__codetermCanvasScene?.get("workspace:workspace")?.nodeScreenBox(FIXTURE_TASK.id), "scene still resolves the focused node");
  assert(!host.querySelector('[data-testid="canvas-breadcrumb"]'), "no breadcrumb bar");
  assert(!host.querySelector('[data-testid="canvas-focus-breadcrumb"]'), "no focus-breadcrumb element");
  await act(async () => root.unmount());
});

test("canvas shows empty state without plans", async () => {
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[]} />);
  assert(host.querySelector('[data-testid="canvas-empty"]'), "empty hint");
  assert(!host.querySelector('[data-testid="canvas-world-canvas"]'), "no world canvas without plans");
  await act(async () => root.unmount());
});

test("canvas scopes workspace plans by project_path", async () => {
  const planA = { ...FIXTURE_PLAN, id: "plan-a", project_path: "/repo/a", title: "Plan A" };
  const { root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[planA]} projectPath="/repo/a" />);
  await paintFrames();
  const titles = readback().filter((n) => n.kind === "plan").map((n) => n.title);
  assert(titles.includes("Plan A"), "plan A rendered");
  assert(!titles.includes("Plan B"), "plan B not rendered when scoped to A");
  await act(async () => root.unmount());

  const planB = { ...FIXTURE_PLAN, id: "plan-b", project_path: "/repo/b", title: "Plan B", tasks: [], sections: [] };
  const { root: root2 } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[planB]} projectPath="/repo/b" />);
  await paintFrames();
  assert(readback().some((n) => n.kind === "plan" && n.title === "Plan B"), "plan B rendered for its project");
  await act(async () => root2.unmount());
});

test("canvas reflects in-place task-data growth (more nodes drawn)", async () => {
  const plans: Plan[] = [{ ...FIXTURE_PLAN, tasks: [{ ...FIXTURE_TASK, body: "small" }] }];
  const { root, rerender } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={plans} />);
  await paintFrames();
  const before = readback().filter((n) => n.kind === "task").length;

  plans[0] = {
    ...plans[0],
    updated_at: plans[0].updated_at + 1,
    tasks: [
      { ...FIXTURE_TASK, id: "task-dante", body: "small" },
      { ...FIXTURE_TASK, id: "task-extra-1", name: "Extra", body: "y".repeat(700), section_ids: ["sec-auth"] },
      { ...FIXTURE_TASK, id: "task-extra-2", name: "More", body: "z".repeat(700), section_ids: ["sec-auth"] },
    ],
  };
  await rerender(<CanvasViewInner {...CANVAS_PROPS} plans={plans} />);
  await paintFrames();
  const after = readback().filter((n) => n.kind === "task").length;
  assert(after > before, `task node count grows (${before} -> ${after})`);
  assert(readback().some((n) => n.id === "task-extra-1"), "new task node drawn");
  await act(async () => root.unmount());
});

test("canvas spec node reflects spec sections loaded from the API", async () => {
  const plans: Plan[] = [{ ...FIXTURE_PLAN, primary_spec_id: "spec-entity-1", sections: [FIXTURE_SECTION] }];
  const originalFetch = globalThis.fetch;
  let releaseSpec!: () => void;
  const specResponse = new Promise<void>((resolve) => { releaseSpec = resolve; });
  let specRequested = false;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: muxMockFetch(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/api/specs/spec-entity-1")) {
        specRequested = true;
        await specResponse;
        return new Response(
          JSON.stringify({
            spec: { sections: [FIXTURE_SECTION, { ...FIXTURE_SECTION, id: "sec-extra", title: "Extra section", subsections: [] }] },
          }),
          { status: 200 },
        );
      }
      assert(url.includes("/reconcile"), `unexpected API request: ${url}`);
      return new Response(JSON.stringify({ drift: [] }), { status: 200 });
    }),
  });
  const api = { enabled: true, host: "127.0.0.1", port: 7686, token: "t", cors: true };
  const { root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={plans} api={api} />);
  try {
    await paintFrames();
    assert(specRequested, "the canvas requests its own spec");
    const assertLoadedSections = () => assert(nodeOf("spec")?.sectionTitles.includes("Extra section"), "loaded spec sections appear in the spec card");
    throws(assertLoadedSections, /loaded spec sections appear in the spec card/);
    await act(async () => { releaseSpec(); await specResponse; });
    await paintFrames();
    assertLoadedSections();
    console.log("  sensitivity: loaded-section oracle RED before response, GREEN after response");
  } finally {
    Object.defineProperty(globalThis, "fetch", { configurable: true, value: originalFetch });
    await act(async () => root.unmount());
  }
});

const OTHER_WS_ID = "ws-other";
const TWO_WORKSPACES: Workspace[] = [
  ...WORKSPACES,
  { id: OTHER_WS_ID, name: "Other", tabs: ["orch-other"], active_tab_id: null, cwd: null },
];
const FIXTURE_OWNED_PLAN: Plan = {
  ...FIXTURE_PLAN,
  id: "perf_plan",
  owner: { tab_id: "perf_orch", session_id: null, title: null },
};
const FOREIGN_PLAN: Plan = {
  ...FIXTURE_PLAN,
  id: "foreign_plan",
  owner: { tab_id: "orch-other", session_id: null, title: null },
};
const scopeOpts = (workspaces: Workspace[]) => ({ workspaceId: WS_ID, workspaces, teams: [], tabs: {} });

// NEGATIVE CONTROL: no fixture active must leave workspace scoping exactly as it was.
test("no fixture active: workspaces are untouched and a foreign-workspace plan is still filtered out", () => {
  const bound = bindFixtureOwnersToWorkspace(TWO_WORKSPACES, WS_ID, fixtureOwnerTabIds([], [FOREIGN_PLAN]));
  assert(bound === TWO_WORKSPACES, "no fixture owners → the same workspaces array is handed down");
  const kept = filterPlansForCanvas([FOREIGN_PLAN], "workspace", scopeOpts(bound));
  assert(kept.length === 0, `foreign-workspace plan must not reach the canvas, got ${kept.length}`);
});

test("fixture active: the fixture plan is admitted and a foreign-workspace plan is still filtered out", () => {
  const ownerIds = fixtureOwnerTabIds([FIXTURE_OWNED_PLAN], [FIXTURE_OWNED_PLAN, FOREIGN_PLAN]);
  assert(ownerIds.join(",") === "perf_orch", `fixture owner ids, got ${ownerIds.join(",")}`);
  const bound = bindFixtureOwnersToWorkspace(TWO_WORKSPACES, WS_ID, ownerIds);
  const kept = filterPlansForCanvas([FIXTURE_OWNED_PLAN, FOREIGN_PLAN], "workspace", scopeOpts(bound));
  assert(kept.length === 1 && kept[0].id === "perf_plan", `only the fixture plan is scoped in, got ${kept.map((p) => p.id).join(",")}`);
  const other = TWO_WORKSPACES.find((w) => w.id === OTHER_WS_ID)!;
  const boundOther = bound.find((w) => w.id === OTHER_WS_ID)!;
  assert(boundOther.tabs.join(",") === other.tabs.join(","), "other workspaces keep their own tab list");
});

test("fixture plans that the shell did not receive contribute no owner binding", () => {
  const ownerIds = fixtureOwnerTabIds([FIXTURE_OWNED_PLAN], [FOREIGN_PLAN]);
  assert(ownerIds.length === 0, `unshown fixture plans bind nothing, got ${ownerIds.join(",")}`);
});

async function main() {
  console.log(`Timing: main entry=${performance.now().toFixed(3)}ms since process time origin`);
  const bodyStarted = performance.now();
  setupDom();
  let passed = 0;
  for (const [name, fn] of tests) {
    const started = performance.now();
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name} body=${(performance.now() - started).toFixed(3)}ms`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
      process.exit(1);
    }
  }
  console.log(`Timing: all bodies=${(performance.now() - bodyStarted).toFixed(3)}ms`);
  console.log(`\n✓ ${passed}/${tests.length} CanvasView tests passed`);
}

main();
