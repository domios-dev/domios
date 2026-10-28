// Run: npx tsx src/components/Canvas/useCanvasViewport.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { useCanvasViewport, AUTO_FIT_MAX_SCALE, CANVAS_ZOOM_INTENSITY, CANVAS_MIN_SCALE, CANVAS_MAX_SCALE, formatLayerTransform } from "@codeterm/canvas-camera";

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
  const { transform, handlers, containerRef, layerRef } = useCanvasViewport({ width: 800, height: 600 });
  last = transform;
  return (
    <div
      ref={containerRef}
      data-testid="vp"
      onMouseDown={handlers.onMouseDown}
      style={{ width: 800, height: 600 }}
    >
      <div ref={layerRef} data-testid="layer" style={{ transform: formatLayerTransform(transform) }} />
    </div>
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
    await new Promise((r) => setTimeout(r, 180));
  });
}

function scaleFromTransform(style: string): number {
  const m = /scale\(([\d.]+)\)/.exec(style);
  return m ? Number(m[1]) : NaN;
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
  const { transform, handlers, containerRef, layerRef } = useCanvasViewport({ width: 800, height: 600 });
  last = transform;
  return (
    <div ref={containerRef} data-testid="vp" onMouseDown={handlers.onMouseDown} style={{ width: 800, height: 600 }}>
      <div ref={layerRef} data-testid="layer" style={{ transform: formatLayerTransform(transform) }} />
      <div data-testid="body" data-canvas-scroll-body="1" style={{ width: 200, height: 100 }} />
    </div>
  );
}

function mockScroll(el: Element, scrollTop: number, clientHeight: number, scrollHeight: number) {
  Object.defineProperty(el, "scrollTop", { value: scrollTop, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: clientHeight, configurable: true });
  Object.defineProperty(el, "scrollHeight", { value: scrollHeight, configurable: true });
}

// §24.2: a node body owns the wheel — scroll ONLY that node, NEVER pan.
test("wheel over a node body scrolls only the node, never pans the canvas", async () => {
  const { host, root } = await render(<ScrollHarness />);
  const body = host.querySelector('[data-testid="body"]')!;
  mockScroll(body, 0, 100, 300);
  wheel(body, { deltaY: 80 });
  await flush();
  assert(last.x === 0 && last.y === 0, `node owns the wheel, canvas did not pan; got ${last.x},${last.y}`);
  await act(async () => root.unmount());
});

test("wheel over a node body AT its scroll boundary STILL does not pan (no hand-off, §24.2)", async () => {
  const { host, root } = await render(<ScrollHarness />);
  const body = host.querySelector('[data-testid="body"]')!;
  mockScroll(body, 200, 100, 300); // at the bottom boundary
  wheel(body, { deltaY: 80 });
  await flush();
  assert(last.x === 0 && last.y === 0, `boundary still never releases to canvas pan; got ${last.x},${last.y}`);
  await act(async () => root.unmount());
});

test("wheel over empty canvas (not a node body) PANS — move OFF the node to pan", async () => {
  const { host, root } = await render(<ScrollHarness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  wheel(vp, { deltaY: 80 });
  await flush();
  assert(last.y === -80, `off a node, the canvas pans; got y=${last.y}`);
  await act(async () => root.unmount());
});

test("rapid pinch (ctrl-wheel) events compound scale before RAF flush", async () => {
  const { host, root } = await render(<Harness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  const layer = host.querySelector('[data-testid="layer"]') as HTMLElement;
  wheel(vp, { deltaY: -40, ctrlKey: true });
  wheel(vp, { deltaY: -40, ctrlKey: true });
  wheel(vp, { deltaY: -40, ctrlKey: true });
  const expected = Math.exp(40 * 3 * CANVAS_ZOOM_INTENSITY);
  assert(Math.abs(scaleFromTransform(layer.style.transform) - expected) < 1e-6, "layer compounded before flush");
  assert(last.scale === 1, "React state deferred until RAF");
  await flush();
  assert(Math.abs(last.scale - expected) < 1e-6, `React state matches compounded scale after flush`);
  await act(async () => root.unmount());
});

test("pinch zoom paints layer transform immediately (not only after RAF)", async () => {
  const { host, root } = await render(<Harness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  const layer = host.querySelector('[data-testid="layer"]') as HTMLElement;
  wheel(vp, { deltaY: -100, ctrlKey: true });
  const factor = Math.exp(100 * CANVAS_ZOOM_INTENSITY);
  assert(Math.abs(scaleFromTransform(layer.style.transform) - factor) < 1e-6, "layer scale painted sync");
  assert(last.scale === 1, "React state still at initial scale before flush");
  await flush();
  assert(Math.abs(last.scale - factor) < 1e-6, "React state catches up after RAF");
  await act(async () => root.unmount());
});

test("zoom cannot go below CANVAS_MIN_SCALE floor", async () => {
  function FloorHarness() {
    const { transform, handlers, containerRef, layerRef } = useCanvasViewport({ width: 800, height: 600, minScale: CANVAS_MIN_SCALE });
    last = transform;
    return (
      <div ref={containerRef} data-testid="vp" onMouseDown={handlers.onMouseDown} style={{ width: 800, height: 600 }}>
        <div ref={layerRef} data-testid="layer" style={{ transform: formatLayerTransform(transform) }} />
      </div>
    );
  }
  const { host, root } = await render(<FloorHarness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  for (let i = 0; i < 40; i++) wheel(vp, { deltaY: 200, ctrlKey: true });
  await flush();
  assert(last.scale >= CANVAS_MIN_SCALE - 1e-6, `scale clamped at floor ${CANVAS_MIN_SCALE}, got ${last.scale}`);
  await act(async () => root.unmount());
});

test("zoom cannot exceed CANVAS_MAX_SCALE ceiling", async () => {
  function CeilingHarness() {
    const { transform, handlers, containerRef, layerRef } = useCanvasViewport({
      width: 800, height: 600, minScale: CANVAS_MIN_SCALE, maxScale: CANVAS_MAX_SCALE,
    });
    last = transform;
    return (
      <div ref={containerRef} data-testid="vp" onMouseDown={handlers.onMouseDown} style={{ width: 800, height: 600 }}>
        <div ref={layerRef} data-testid="layer" style={{ transform: formatLayerTransform(transform) }} />
      </div>
    );
  }
  const { host, root } = await render(<CeilingHarness />);
  const vp = host.querySelector('[data-testid="vp"]')!;
  for (let i = 0; i < 60; i++) wheel(vp, { deltaY: -200, ctrlKey: true });
  await flush();
  assert(last.scale <= CANVAS_MAX_SCALE + 1e-6, `scale clamped at ceiling ${CANVAS_MAX_SCALE}, got ${last.scale}`);
  await act(async () => root.unmount());
});

test("an automatic fit of small content never zooms in past 100%; a fit the viewer asks for still may", async () => {
  let fit: ((bounds: { x: number; y: number; width: number; height: number }, options?: { maxScale?: number }) => void) | null = null;
  function FitHarness() {
    const viewport = useCanvasViewport({ width: 800, height: 600, maxScale: 2.5 });
    fit = viewport.fitToView;
    last = viewport.transform;
    return <div ref={viewport.containerRef} style={{ width: 800, height: 600 }}><div ref={viewport.layerRef} /></div>;
  }
  const { root } = await render(<FitHarness />);
  // A card still loading: a layout far smaller than the viewport.
  const small = { x: 0, y: 0, width: 200, height: 120 };
  await act(async () => fit!(small, { maxScale: AUTO_FIT_MAX_SCALE }));
  assert(last.scale === 1, `auto fit should hold at 100%, got ${last.scale}`);
  await act(async () => fit!(small));
  assert(last.scale > 1, `an explicit fit may zoom in, got ${last.scale}`);
  await act(async () => fit!({ x: 0, y: 0, width: 4000, height: 3000 }, { maxScale: AUTO_FIT_MAX_SCALE }));
  assert(last.scale < 1, `auto fit still zooms out to fit, got ${last.scale}`);
  await act(async () => root.unmount());
});

test("CANVAS_MIN_SCALE is 10% and CANVAS_MAX_SCALE is 400%", () => {
  assert(CANVAS_MIN_SCALE === 0.1, `min ${CANVAS_MIN_SCALE}`);
  assert(CANVAS_MAX_SCALE === 4, `max ${CANVAS_MAX_SCALE}`);
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
