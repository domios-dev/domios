// Run: npx tsx src/components/Canvas/canvasView/useCanvasOverlays.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { CanvasScene } from "../scene";
import { FIXTURE_TASK } from "../cards/fixtures.testdata";
import type { LaidOutNode } from "../layout";
import type { TaskCardModel, CanvasLookups } from "./models";
import { useCanvasOverlays } from "./useCanvasOverlays";

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
  const window = new Window({ url: "http://localhost/canvas-overlays" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: ReactElement): Promise<{ root: Root; rerender: (ui: ReactElement) => Promise<void> }> {
  const host = document.createElement("div");
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
  return { root, rerender };
}

function churn() {
  return window.__codetermCanvasChurn?.snapshot() ?? null;
}

const node: LaidOutNode = {
  id: FIXTURE_TASK.id,
  kind: "task",
  planId: "plan-alpha",
  box: { x: 100, y: 80, width: 220, height: 120 },
  iteration: null,
  manual: false,
};

const taskModel: TaskCardModel = {
  task: FIXTURE_TASK,
  childCount: 0,
  childDone: 0,
  parentTaskName: null,
  stepColors: new Map(),
  presence: {
    tabId: "tab-1",
    label: "Worker",
    agents: [{ id: "tab-1", label: "Worker", role: "worker", color: "#4f7df0" }],
    latest: null,
    focus: null,
    branch: "lane/test",
  },
};

const lookups: CanvasLookups = {
  nodeById: new Map([[node.id, node]]),
  taskById: new Map([[FIXTURE_TASK.id, FIXTURE_TASK]]),
  taskModelById: new Map([[FIXTURE_TASK.id, taskModel]]),
  sectionById: new Map(),
  milestoneTaskById: new Map(),
  elementById: new Map(),
};

const scene: CanvasScene = {
  nodes: [],
  edges: [],
  groups: [],
  bounds: { x: 0, y: 0, width: 500, height: 400 },
};

const visibleIds = new Set([node.id]);
const hiddenIds = new Set<string>();
const worldByNode = new Map([[node.id, "W1" as const]]);

interface HarnessProps {
  x: number;
  y: number;
  scale: number;
  visible: boolean;
}

function Harness({ x, y, scale, visible }: HarnessProps) {
  const { presenceOverlays } = useCanvasOverlays({
    scene,
    transform: { x, y, scale },
    focusedNodeId: null,
    mode: "workspace",
    lookups,
    nodeIds: visible ? visibleIds : hiddenIds,
    worldByNode,
  });
  return <div data-count={presenceOverlays.length} />;
}

test("presence overlay churn ignores x/y-only pan and rebuilds on scale or visibility", async () => {
  setupDom();
  const { root, rerender } = await render(<Harness x={0} y={0} scale={1} visible />);
  window.__codetermCanvasChurn?.reset();

  await rerender(<Harness x={120} y={64} scale={1} visible />);
  const afterPan = churn();
  assert(afterPan?.presenceOverlayBuilds === 0, `pan-only overlay builds: ${afterPan?.presenceOverlayBuilds}`);
  assert(afterPan?.sceneReadbackPublishes === 1, "pan still updates scene readback");

  await rerender(<Harness x={120} y={64} scale={1.25} visible />);
  const afterScale = churn();
  assert(afterScale?.presenceOverlayBuilds === 1, `scale overlay builds: ${afterScale?.presenceOverlayBuilds}`);

  window.__codetermCanvasChurn?.reset();
  await rerender(<Harness x={120} y={64} scale={1.25} visible={false} />);
  const afterVisibility = churn();
  assert(afterVisibility?.presenceOverlayBuilds === 1, `visibility overlay builds: ${afterVisibility?.presenceOverlayBuilds}`);

  await act(async () => root.unmount());
});

async function main() {
  let passed = 0;
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
      passed++;
    } catch (error) {
      console.error(`  ✗ ${name}`);
      console.error(error);
      failed++;
    }
  }
  console.log(`\nuseCanvasOverlays: ${passed} passed, ${failed} failed`);
  process.exit(failed === 0 ? 0 : 1);
}

void main();
