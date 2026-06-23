// Run: npx tsx src/components/Canvas/CanvasView.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { CanvasViewInner } from "./CanvasView";
import { registerNodeRenderer } from "./nodeRegistry";
import type { CanvasElement, Plan } from "./types";

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
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  host.style.width = "1200px";
  host.style.height = "800px";
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

test("CanvasViewInner renders plan frame and task card from fixture", async () => {
  const { host, root } = await render(
    <CanvasViewInner mode="workspace" plans={[FIXTURE_PLAN]} viewportPadding={10000} />,
  );
  await act(async () => {
    await new Promise((r) => setTimeout(r, 10));
  });
  assert(host.querySelector('[data-testid="canvas-view"]'), "canvas shell");
  assert(host.querySelector('[data-testid="canvas-plan-frame"]'), "plan frame");
  assert(host.querySelector('[data-testid="canvas-section-card"]'), "section card");
  assert(host.querySelector('[data-testid="canvas-task-card"]'), "task card");
  assert(host.querySelector('[data-testid="canvas-task-name"]')?.textContent === "Dante", "task name");
  await act(async () => root.unmount());
});

function elementAt(id: string, kind: string, x: number, y: number): CanvasElement {
  return { id, kind, canvas: { x, y, collapsed: false }, content: `**${kind}** body`, links: [] };
}

// Extensibility gate (§6.3): a NEW kind plugs in via one registerNodeRenderer
// call — CanvasView dispatches it through the registry with ZERO core edits —
// and an unregistered kind safely hits the fallback card (no crash).
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
    <CanvasViewInner mode="workspace" plans={[plan]} viewportPadding={10000} />,
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
  const { host, root } = await render(<CanvasViewInner mode="workspace" plans={[]} />);
  assert(host.querySelector('[data-testid="canvas-empty"]'), "empty hint");
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
