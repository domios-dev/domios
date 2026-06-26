// Run: npx tsx src/components/Canvas/CanvasView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import type { Pane, Workspace } from "../../lib/types";
import { FIXTURE_PLAN, FIXTURE_SECTION, FIXTURE_TASK } from "./cards/fixtures.testdata";
import { CanvasViewInner } from "./CanvasView";
import { registerNodeRenderer } from "./nodeRegistry";
import { __resetSubstrateV2ForTest } from "./substrateFlag";
import { __resetOverlayBudgetForTest } from "./overlayBudgetFlag";
import type { CanvasElement, Plan } from "./types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const STABLE_PANES = {};
const CANVAS_PROPS = { mode: "workspace" as const, panes: STABLE_PANES, viewportPadding: 10000 };

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/canvas-view" });
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
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
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

const FIXTURE_PLAN_WITH_SPEC: Plan = {
  ...FIXTURE_PLAN,
  primary_spec_id: "spec-fixture",
};

test("CanvasViewInner renders the SEPARATE spec + plan nodes (§24.3) and a task card", async () => {
  const { host, root } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN_WITH_SPEC]} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  assert(host.querySelector('[data-testid="canvas-view"]'), "canvas shell");
  assert(host.querySelector('[data-testid="canvas-plan-node"]'), "plan node present");
  assert(host.querySelector('[data-testid="canvas-spec-node"]'), "separate spec node present");
  assert(host.querySelector('[data-testid="canvas-plan-row"]'), "plan section rows");
  assert(host.querySelector('[data-testid="canvas-spec-block"]'), "spec section blocks");
  assert(host.querySelector('[data-testid="canvas-plan-output"]'), "plan output dot (task anchor)");
  assert(host.querySelector('[data-testid="canvas-task-card"]'), "task card");
  assert(host.querySelector('[data-testid="canvas-task-name"]')?.textContent === "Dante", "task name");
  assert(host.querySelector('[data-testid="canvas-orchestrator-group"]'), "orchestrator group wrapper");
  const paths = host.querySelectorAll("svg path");
  assert(paths.length >= 2, `spec↔plan + section→task edges drawn at rest, got ${paths.length}`);
  await act(async () => root.unmount());
});

test("CanvasViewInner derives task presence from panes heartbeat and latest stride", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    heartbeat: {
      current_focus: "Implement presence chip",
      active_lanes: ["task-dante"],
      rules: [],
      learnings: [],
    },
    tasks: [
      {
        ...FIXTURE_TASK,
        assigned_pane_id: "pane-42",
        milestones: [
          {
            id: "stride-1",
            from: { pane_id: "pane-42", session_id: null, title: null },
            kind: "status_update",
            text: "Presence marker wired.",
            ts: 1_700_000_200_000,
            links: [],
            loop_id: null,
            iteration: null,
            canvas: { x: null, y: null, collapsed: false },
          },
        ],
      },
    ],
  };
  const panes: Record<string, Pane> = {
    "pane-42": {
      id: "pane-42",
      pty_id: null,
      pane_type: "Terminal",
      title: "Puccini",
      cwd: "/tmp/project",
      agent_type: "codex",
    },
  };
  const { host, root } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={[plan]} panes={panes} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  const marker = host.querySelector('[data-testid="canvas-task-presence"]') as HTMLElement | null;
  assert(marker, "presence marker rendered on task card");
  assert(marker.textContent?.includes("Puccini"), `pane title used as assignee label, got ${marker?.textContent}`);
  assert(marker.textContent?.includes("Implement presence chip"), `heartbeat focus visible, got ${marker?.textContent}`);
  assert(marker.textContent?.includes("status update"), `latest stride kind visible, got ${marker?.textContent}`);
  await act(async () => root.unmount());
});

test("CanvasViewInner labels cross-workspace plans with owner workspace", async () => {
  const plan: Plan = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "task-api", assigned_pane_id: "pane-api" },
      {
        ...FIXTURE_TASK,
        id: "task-ui",
        assigned_pane_id: "pane-ui",
        lane: { pane_id: "pane-ui", workspace_id: "ws-ui", workspace_name: "UI Lane" },
      },
    ],
  };
  const workspaces: Workspace[] = [
    { id: "ws-owner", name: "Canvas Hub", tabs: [{ id: "tab-owner", pane_id: "orch-1" }], active_tab_id: null, cwd: "/repo/hub" },
    { id: "ws-api", name: "API Lane", tabs: [{ id: "tab-api", pane_id: "pane-api" }], active_tab_id: null, cwd: "/repo/api" },
    { id: "ws-ui", name: "UI Lane", tabs: [{ id: "tab-ui", pane_id: "pane-ui" }], active_tab_id: null, cwd: "/repo/ui" },
  ];

  const { host, root } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={[plan]} workspaces={workspaces} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  const owner = host.querySelector('[data-testid="canvas-plan-owner"]');
  assert(owner?.textContent?.includes("Canvas Hub"), `owner workspace label visible, got ${owner?.textContent}`);
  const badge = host.querySelector('[data-testid="canvas-plan-cross-workspace"]') as HTMLElement | null;
  assert(badge, "cross-workspace badge rendered on plan node");
  assert(badge.title.includes("API Lane") && badge.title.includes("UI Lane"), `participant workspaces in tooltip, got ${badge.title}`);
  await act(async () => root.unmount());
});

test("double-click camera-focuses a node (breadcrumb appears), Esc clears it", async () => {
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN]} />);
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });
  const planNode = host.querySelector('[data-testid="canvas-plan-node"]') as HTMLElement;
  assert(!host.querySelector('[data-testid="canvas-breadcrumb"]'), "no breadcrumb before focus");
  await act(async () => {
    planNode.dispatchEvent(new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("dblclick", { bubbles: true }));
  });
  assert(host.querySelector('[data-testid="canvas-breadcrumb"]'), "breadcrumb shows the focused node");
  assert(host.querySelector('[data-testid="canvas-breadcrumb"]')?.textContent?.includes("Auth rollout"), "breadcrumb names the focused plan");
  await act(async () => root.unmount());
});

test("substrate v2 double-click focuses by scene geometry when DOM target has no node id", async () => {
  __resetSubstrateV2ForTest();
  window.localStorage.setItem("canvas.substrateV2", "true");
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN]} />);
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });

  const readback = window.__codetermCanvasScene?.get("workspace:workspace");
  const screenBox = readback?.nodeScreenBox(FIXTURE_PLAN.id);
  const layer = host.querySelector('[data-testid="canvas-world-layer"]') as HTMLElement;
  assert(screenBox, "scene readback exposes plan screen box");
  assert(layer, "world layer present");

  await act(async () => {
    layer.dispatchEvent(new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("dblclick", {
      bubbles: true,
      clientX: screenBox.x + screenBox.width / 2,
      clientY: screenBox.y + screenBox.height / 2,
    }));
  });
  assert(host.querySelector('[data-testid="canvas-breadcrumb"]')?.textContent?.includes("Auth rollout"), "scene geometry picked and focused plan");
  const focusedReadback = window.__codetermCanvasScene?.get("workspace:workspace");
  const overlays = focusedReadback?.activeOverlays() ?? [];
  assert(overlays.length === 1 && overlays[0].id === FIXTURE_PLAN.id, "focused node publishes one rich overlay");
  assert(focusedReadback?.maxActiveOverlays === 2, "readback publishes configured rich overlay budget");
  window.localStorage.removeItem("canvas.substrateV2");
  __resetSubstrateV2ForTest();
  await act(async () => root.unmount());
});

function elementAt(id: string, kind: string, x: number, y: number): CanvasElement {
  return { id, kind, canvas: { x, y, collapsed: false }, content: `**${kind}** body`, links: [] };
}

test("CanvasViewInner renders a registered new kind via the registry and falls back on unknown kinds", async () => {
  registerNodeRenderer("note", ({ node, style }) => (
    <div data-testid="canvas-note-element" data-id={node.id} style={style}>
      sticky note
    </div>
  ));

  const plan: Plan = {
    ...FIXTURE_PLAN,
    id: "plan-ext",
    elements: [
      elementAt("el-note", "note", 600, 40),
      elementAt("el-mystery", "mystery-kind", 600, 200),
    ],
  };

  const { host, root } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={[plan]} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  assert(host.querySelector('[data-testid="canvas-note-element"]'), "note kind dispatched via registry");
  const fb = host.querySelector('[data-testid="canvas-fallback-node"]');
  assert(fb, "unknown kind hit the fallback card");
  assert(fb?.getAttribute("data-node-kind") === "mystery-kind", "fallback carries the unknown kind");
  await act(async () => root.unmount());
});

test("CanvasViewInner shows empty state without plans", async () => {
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[]} />);
  assert(host.querySelector('[data-testid="canvas-empty"]'), "empty hint");
  await act(async () => root.unmount());
});

test("CanvasViewInner scopes workspace plans by project_path from fetch", async () => {
  const planA = { ...FIXTURE_PLAN, id: "plan-a", project_path: "/repo/a", title: "Plan A" };
  const planB = {
    ...FIXTURE_PLAN,
    id: "plan-b",
    project_path: "/repo/b",
    title: "Plan B",
    tasks: [],
    sections: [],
  };

  const { host, root } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={[planA]} projectPath="/repo/a" />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  assert(host.querySelector('[data-testid="canvas-plan-node"]'), "plan A frame visible");
  const titles = [...host.querySelectorAll('[data-testid="canvas-plan-node"]')].map(
    (el) => el.textContent ?? "",
  );
  assert(!titles.some((t) => t.includes("Plan B")), "plan B not rendered when scoped to A");
  assert(titles.some((t) => t.includes("Plan A")), "plan A rendered");

  await act(async () => root.unmount());

  const { host: host2, root: root2 } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={[planB]} projectPath="/repo/b" />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  const titlesB = [...host2.querySelectorAll('[data-testid="canvas-plan-node"]')].map(
    (el) => el.textContent ?? "",
  );
  assert(titlesB.some((t) => t.includes("Plan B")), "plan B rendered for its project");
  await act(async () => root2.unmount());
});

test("CanvasViewInner recomputes world bounds when plan task data changes in place", async () => {
  const plans: Plan[] = [
    {
      ...FIXTURE_PLAN,
      tasks: [{ ...FIXTURE_TASK, body: "small" }],
    },
  ];
  const { host, root, rerender } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={plans} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  const layer = () => host.querySelector('[data-testid="canvas-world-layer"]') as HTMLElement;
  const rev0 = layer()?.getAttribute("data-layout-revision") ?? "";
  const w0 = Number.parseFloat(layer()?.style.width ?? "0");
  const h0 = Number.parseFloat(layer()?.style.height ?? "0");

  plans[0] = {
    ...plans[0],
    updated_at: plans[0].updated_at + 1,
    tasks: [
      { ...FIXTURE_TASK, id: "task-dante", body: "small" },
      {
        ...FIXTURE_TASK,
        id: "task-extra-1",
        name: "Extra",
        body: "y".repeat(700),
        section_ids: ["sec-auth"],
      },
      {
        ...FIXTURE_TASK,
        id: "task-extra-2",
        name: "More",
        body: "z".repeat(700),
        section_ids: ["sec-auth"],
      },
    ],
  };

  await rerender(<CanvasViewInner {...CANVAS_PROPS} plans={plans} />);
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });

  const rev1 = layer()?.getAttribute("data-layout-revision") ?? "";
  const w1 = Number.parseFloat(layer()?.style.width ?? "0");
  const h1 = Number.parseFloat(layer()?.style.height ?? "0");
  assert(rev0 !== rev1, "layout revision advances on task growth");
  assert(w1 > w0 || h1 > h0, `world bounds grow (${w0}x${h0} -> ${w1}x${h1})`);
  await act(async () => root.unmount());
});

test("CanvasViewInner recomputes layout revision when spec sections load", async () => {
  const plans: Plan[] = [{ ...FIXTURE_PLAN, primary_spec_id: "spec-entity-1", sections: [FIXTURE_SECTION] }];
  const originalFetch = globalThis.fetch;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/api/specs/spec-entity-1")) {
        return new Response(
          JSON.stringify({
            spec: {
              sections: [
                FIXTURE_SECTION,
                { ...FIXTURE_SECTION, id: "sec-extra", title: "Extra section", subsections: [] },
              ],
            },
          }),
          { status: 200 },
        );
      }
      return originalFetch(input);
    },
  });

  const api = { enabled: true, host: "127.0.0.1", port: 7686, token: "t", cors: true };
  const { host, root } = await render(
    <CanvasViewInner {...CANVAS_PROPS} plans={plans} api={api} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });
  const rev = host.querySelector('[data-testid="canvas-world-layer"]')?.getAttribute("data-layout-revision") ?? "";
  assert(rev.includes("sec-extra"), "loaded spec sections participate in layout revision");
  Object.defineProperty(globalThis, "fetch", { configurable: true, value: originalFetch });
  await act(async () => root.unmount());
});

test("substrate v2 mounts focused rich overlay at scene screenRect", async () => {
  __resetSubstrateV2ForTest();
  window.localStorage.setItem("canvas.substrateV2", "true");
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN]} />);
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });

  const planNode = host.querySelector('[data-testid="canvas-plan-node"]') as HTMLElement;
  await act(async () => {
    planNode.dispatchEvent(new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("dblclick", { bubbles: true }));
  });

  const overlay = host.querySelector('[data-testid="canvas-rich-overlay"]') as HTMLElement | null;
  assert(overlay, "focused rich overlay mounted");
  assert(overlay.getAttribute("data-overlay-id") === FIXTURE_PLAN.id, "overlay targets focused plan");
  const readback = window.__codetermCanvasScene?.get("workspace:workspace");
  const screenBox = readback?.nodeScreenBox(FIXTURE_PLAN.id);
  assert(screenBox, "scene readback exposes plan screen box");
  const left = Number.parseFloat(overlay.style.left);
  const top = Number.parseFloat(overlay.style.top);
  const width = Number.parseFloat(overlay.style.width);
  const height = Number.parseFloat(overlay.style.height);
  assert(Math.abs(left - screenBox!.x) < 1, `overlay left tracks screenRect (${left} ~ ${screenBox!.x})`);
  assert(Math.abs(top - screenBox!.y) < 1, `overlay top tracks screenRect (${top} ~ ${screenBox!.y})`);
  assert(Math.abs(width - screenBox!.width) < 1, `overlay width tracks screenRect (${width} ~ ${screenBox!.width})`);
  assert(Math.abs(height - screenBox!.height) < 1, `overlay height tracks screenRect (${height} ~ ${screenBox!.height})`);

  window.localStorage.removeItem("canvas.substrateV2");
  __resetSubstrateV2ForTest();
  await act(async () => root.unmount());
});

test("overlay budget mounts lightweight world cards and keeps rich chrome in overlay only", async () => {
  __resetSubstrateV2ForTest();
  __resetOverlayBudgetForTest();
  window.localStorage.setItem("canvas.substrateV2", "true");
  window.localStorage.setItem("canvas.overlayBudget", "true");
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN_WITH_SPEC]} />);
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });

  const worldLayer = '[data-testid="canvas-world-layer"]';
  const lightweight = host.querySelectorAll(`${worldLayer} [data-testid="canvas-world-lightweight"]`).length;
  assert(lightweight >= 3, `world layer uses lightweight cards, got ${lightweight}`);
  assert(host.querySelectorAll(`${worldLayer} [data-testid="canvas-plan-row"]`).length === 0, "no rich plan rows in world layer");
  assert(host.querySelectorAll(`${worldLayer} [data-testid="canvas-spec-block"]`).length === 0, "no rich spec blocks in world layer");
  assert(host.querySelectorAll(`${worldLayer} [data-renderer="rich-markdown"]`).length === 0, "no rich markdown in world layer");

  const planNode = host.querySelector(`${worldLayer} [data-plan-id="${FIXTURE_PLAN.id}"]`) as HTMLElement;
  assert(planNode?.getAttribute("data-world-render") === "lightweight", "focused target stays lightweight in world layer");
  await act(async () => {
    planNode.dispatchEvent(new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("dblclick", { bubbles: true }));
  });

  const overlayRich = host.querySelector('[data-testid="canvas-rich-overlay"] [data-testid="canvas-plan-row"], [data-testid="canvas-rich-overlay"] [data-testid="canvas-spec-block"]');
  assert(overlayRich, "focused overlay keeps rich plan/spec chrome");
  assert(host.querySelectorAll(`${worldLayer} [data-testid="canvas-plan-row"]`).length === 0, "world layer stays lightweight after focus");

  window.localStorage.removeItem("canvas.substrateV2");
  window.localStorage.removeItem("canvas.overlayBudget");
  __resetSubstrateV2ForTest();
  __resetOverlayBudgetForTest();
  await act(async () => root.unmount());
});

test("overlay budget suppresses world-layer rich bodies while focused overlay stays rich", async () => {
  __resetSubstrateV2ForTest();
  __resetOverlayBudgetForTest();
  window.localStorage.setItem("canvas.substrateV2", "true");
  window.localStorage.setItem("canvas.overlayBudget", "true");
  const { host, root } = await render(<CanvasViewInner {...CANVAS_PROPS} plans={[FIXTURE_PLAN_WITH_SPEC]} />);
  await act(async () => { await new Promise((r) => setTimeout(r, 10)); });

  assert(host.querySelector('[data-testid="canvas-view"]')?.getAttribute("data-overlay-budget") === "true", "canvas view advertises overlay budget");
  const worldLayer = '[data-testid="canvas-world-layer"]';
  const worldRichSelector = `${worldLayer} [data-testid="canvas-plan-section-body"], ${worldLayer} [data-testid="canvas-spec-section-body"], ${worldLayer} [data-testid="canvas-task-full-body"]`;
  const worldRichBefore = host.querySelectorAll(worldRichSelector).length;
  assert(worldRichBefore === 0, `world layer rich bodies suppressed before focus, got ${worldRichBefore}`);

  const planNode = host.querySelector(`${worldLayer} [data-plan-id="${FIXTURE_PLAN.id}"]`) as HTMLElement;
  assert(planNode, "plan node present in world layer for focus gesture");
  await act(async () => {
    planNode.dispatchEvent(new (window as unknown as { MouseEvent: typeof MouseEvent }).MouseEvent("dblclick", { bubbles: true }));
  });

  const overlayRich = host.querySelector('[data-testid="canvas-rich-overlay"] [data-testid="canvas-plan-section-body"], [data-testid="canvas-rich-overlay"] [data-testid="canvas-spec-section-body"], [data-testid="canvas-rich-overlay"] [data-testid="canvas-task-full-body"]');
  assert(overlayRich, "focused overlay keeps rich section/body content");
  const worldRichAfter = host.querySelectorAll(worldRichSelector).length;
  assert(worldRichAfter === 0, `world layer stays degraded after focus, got ${worldRichAfter}`);

  window.localStorage.removeItem("canvas.substrateV2");
  window.localStorage.removeItem("canvas.overlayBudget");
  __resetSubstrateV2ForTest();
  __resetOverlayBudgetForTest();
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
  console.log(`\n✓ ${passed}/${tests.length} CanvasView tests passed`);
}

main();
