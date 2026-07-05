// Run: npx tsx src/components/Canvas/missionControl/FlowCurves.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FlowCurves } from "./FlowCurves";
import { edgesFromRects, type MeasuredEdge, type StationRect } from "./flowLayout";

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
  const window = new Window({ url: "http://localhost/flow-curves" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: ReactElement) {
  setupDom();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, unmount: async () => { await act(async () => root.unmount()); host.remove(); } };
}

// F2: edges derived from measured station rects, not fixed pixel paths.
test("edgesFromRects connects consecutive stations at their real edges", () => {
  const rects: StationRect[] = [
    { key: "frontier", left: 0, top: 20, width: 100, height: 80 },
    { key: "milestone", left: 200, top: 20, width: 120, height: 90 },
    { key: "verify", left: 400, top: 20, width: 100, height: 70 },
  ];
  const edges = edgesFromRects(rects);
  assert(edges.length === 2, `2 edges for 3 stations, got ${edges.length}`);
  // frontier right edge (x=100, y=60) → milestone left edge (x=200, y=65)
  assert(edges[0].d.startsWith("M100 60"), `edge starts at frontier right-center, got ${edges[0].d}`);
  assert(edges[0].mid.x === 150, `midpoint x between rects, got ${edges[0].mid.x}`);
  assert(edges[1].active, "milestone→verify edge is the active edge");
  assert(!edges[0].active, "frontier→milestone edge is not active");
});

test("edgesFromRects tracks different widths (responsive/zoom-independent)", () => {
  const wide = edgesFromRects([
    { key: "milestone", left: 0, top: 0, width: 300, height: 100 },
    { key: "verify", left: 600, top: 0, width: 100, height: 100 },
  ]);
  assert(wide[0].d.startsWith("M300 50"), `docks to measured right edge (300), got ${wide[0].d}`);
});

function activeEdges(): MeasuredEdge[] {
  return edgesFromRects([
    { key: "milestone", left: 0, top: 0, width: 100, height: 100 },
    { key: "verify", left: 300, top: 0, width: 100, height: 100 },
  ]);
}

test("motion ON: pulse animates on the active edge", async () => {
  const { host, unmount } = await render(<FlowCurves edges={activeEdges()} width={400} height={100} animate />);
  assert(host.querySelector('[data-testid="flow-pulse"]'), "pulse dot rendered");
  assert(host.querySelector('[data-testid="flow-pulse"] animate'), "animate element present under motion");
  await unmount();
});

test("reduced-motion: static pulse, NO <animate> element", async () => {
  const { host, unmount } = await render(<FlowCurves edges={activeEdges()} width={400} height={100} animate={false} />);
  assert(host.querySelector('[data-testid="flow-pulse"]'), "pulse dot still rendered (static)");
  assert(!host.querySelector('[data-testid="flow-pulse"] animate'), "NO animate under reduced-motion");
  await unmount();
});

test("no curves when unmeasured (width 0) — avoids ghost faint edges", async () => {
  const { host, unmount } = await render(<FlowCurves edges={activeEdges()} width={0} height={0} animate />);
  assert(!host.querySelector('[data-testid="flow-curves"]'), "no svg when band unmeasured");
  await unmount();
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed += 1;
      console.error(`  ✗ ${name}: ${(e as Error).message}`);
    }
  }
  if (failed > 0) {
    console.error(`\n${failed} of ${tests.length} FlowCurves test(s) failed`);
    process.exit(1);
  }
  console.log(`\n✓ all ${tests.length} FlowCurves test(s) green`);
}

void main();
