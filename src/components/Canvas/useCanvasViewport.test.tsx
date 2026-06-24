// Run: npx tsx src/components/Canvas/useCanvasViewport.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { useCanvasViewport } from "./useCanvasViewport";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

let win: Window;

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  win = new Window({ url: "http://localhost/viewport" });
  (win as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const key of ["window", "document", "navigator", "HTMLElement", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (win as unknown as Record<string, unknown>)[key], configurable: true });
  }
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
  (globalThis as unknown as { cancelAnimationFrame: (id: number) => void }).cancelAnimationFrame = (id: number) =>
    clearTimeout(id as unknown as ReturnType<typeof setTimeout>);
}

let last = { x: 0, y: 0, scale: 1 };

function Harness() {
  const { transform, handlers, containerRef } = useCanvasViewport({ width: 800, height: 600 });
  last = transform;
  return (
    <div
      ref={containerRef}
      data-testid="vp"
      onMouseDown={handlers.onMouseDown}
      style={{ width: 800, height: 600 }}
    />
  );
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

function wheel(el: Element, init: { deltaX?: number; deltaY?: number; ctrlKey?: boolean; metaKey?: boolean }) {
  const ev = new win.Event("wheel", { bubbles: true, cancelable: true }) as unknown as Record<string, unknown>;
  Object.assign(ev, {
    deltaX: init.deltaX ?? 0,
    deltaY: init.deltaY ?? 0,
    deltaMode: 0,
    ctrlKey: !!init.ctrlKey,
    metaKey: !!init.metaKey,
    clientX: 400,
    clientY: 300,
    preventDefault() {},
  });
  el.dispatchEvent(ev as unknown as Event);
}

async function flush() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 5));
  });
}

test("plain two-finger wheel PANS both axes, scale unchanged", async () => {
  const { host, root } = await render(<Harness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  wheel(vp, { deltaX: 40, deltaY: 100 });
  await flush();
  assert(last.scale === 1, `scale stays 1 on pan, got ${last.scale}`);
  assert(last.x === -40, `x pans by -deltaX, got ${last.x}`);
  assert(last.y === -100, `y pans by -deltaY, got ${last.y}`);
  await act(async () => root.unmount());
});

test("cmd/ctrl wheel ZOOMS around pointer, position pans too", async () => {
  const { host, root } = await render(<Harness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  wheel(vp, { deltaY: -100, ctrlKey: true });
  await flush();
  assert(last.scale > 1, `ctrl+wheel zooms in (scale>1), got ${last.scale}`);
  await act(async () => root.unmount());

  const { host: h2, root: r2 } = await render(<Harness />);
  const vp2 = h2.querySelector('[data-testid="vp"]')!;
  wheel(vp2, { deltaY: 120, metaKey: true });
  await flush();
  assert(last.scale < 1, `meta+wheel zooms out (scale<1), got ${last.scale}`);
  await act(async () => r2.unmount());
});

function ScrollHarness() {
  const { transform, handlers, containerRef } = useCanvasViewport({ width: 800, height: 600 });
  last = transform;
  return (
    <div ref={containerRef} data-testid="vp" onMouseDown={handlers.onMouseDown} style={{ width: 800, height: 600 }}>
      <div data-testid="body" data-canvas-scroll-body="1" style={{ width: 200, height: 100 }} />
    </div>
  );
}

function mockScroll(el: Element, scrollTop: number, clientHeight: number, scrollHeight: number) {
  Object.defineProperty(el, "scrollTop", { value: scrollTop, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: clientHeight, configurable: true });
  Object.defineProperty(el, "scrollHeight", { value: scrollHeight, configurable: true });
}

// §23.6 distance-gated internal scroll.
test("zoomed in: wheel over a scrollable body that CAN scroll is consumed (no pan)", async () => {
  const { host, root } = await render(<ScrollHarness />);
  const body = host.querySelector('[data-testid="body"]')!;
  mockScroll(body, 0, 100, 300); // can scroll DOWN
  wheel(body, { deltaY: 80 }); // scale starts at 1 (>= NEAR_SCROLL_SCALE)
  await flush();
  assert(last.x === 0 && last.y === 0, `body consumed the wheel, canvas did not pan; got ${last.x},${last.y}`);
  await act(async () => root.unmount());
});

test("zoomed in but body AT its boundary hands the wheel back to canvas pan", async () => {
  const { host, root } = await render(<ScrollHarness />);
  const body = host.querySelector('[data-testid="body"]')!;
  mockScroll(body, 200, 100, 300); // scrolled to bottom → cannot scroll DOWN further
  wheel(body, { deltaY: 80 });
  await flush();
  assert(last.y === -80, `at boundary the canvas pans; got y=${last.y}`);
  await act(async () => root.unmount());
});

test("zoomed out: wheel over a scrollable body still PANS (scroll never traps far out)", async () => {
  const { host, root } = await render(<ScrollHarness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  wheel(vp, { deltaY: 300, ctrlKey: true }); // zoom OUT → scale < 1
  await flush();
  assert(last.scale < 1, `precondition: zoomed out, got ${last.scale}`);
  const body = host.querySelector('[data-testid="body"]')!;
  mockScroll(body, 0, 100, 300); // could scroll, but we're zoomed out
  const beforeY = last.y;
  wheel(body, { deltaY: 80 });
  await flush();
  assert(last.y === beforeY - 80, `zoomed out, body wheel pans the canvas; got y=${last.y}`);
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
      console.error(`    ${e instanceof Error ? e.stack : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} useCanvasViewport tests passed`);
}

main();
