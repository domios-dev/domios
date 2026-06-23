// Run: npx tsx src/components/Canvas/useCanvasNodeDrag.test.tsx

import { act, useRef, type MouseEvent as ReactMouseEvent } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { layoutWorkspace } from "./layout";
import type { UseCanvasNodeDragResult } from "./useCanvasNodeDrag";

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
  const window = new Window({ url: "http://127.0.0.1/canvas-drag" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  let rafCb: FrameRequestCallback | null = null;
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => {
      rafCb = cb;
      return 1;
    };
  (globalThis as unknown as { cancelAnimationFrame: () => void }).cancelAnimationFrame = () => {
    rafCb = null;
  };
  (globalThis as unknown as { __flushRaf: () => void }).__flushRaf = () => {
    rafCb?.(performance.now());
    rafCb = null;
  };
}

async function main() {
  setupDom();
  const { useCanvasNodeDrag } = await import("./useCanvasNodeDrag");
  const layout = layoutWorkspace([FIXTURE_PLAN]);
  const taskNode = layout.frames[0]?.nodes.find((n) => n.kind === "task");
  assert(taskNode, "task node in layout");

  test("drag override updates coalesce via requestAnimationFrame", async () => {
    const holder: { current: UseCanvasNodeDragResult | null } = { current: null };
    const transform = { x: 0, y: 0, scale: 1 };

    function DragProbe() {
      const containerRef = useRef<HTMLDivElement | null>(null);
      const state = useCanvasNodeDrag({ containerRef, transform, layout });
      holder.current = state;
      return <div ref={containerRef} className="drag-host" style={{ width: 800, height: 600 }} />;
    }

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(<DragProbe />);
    });

    const pan = host.querySelector(".drag-host") as HTMLElement;
    pan.getBoundingClientRect = () =>
      ({
        left: 0,
        top: 0,
        width: 800,
        height: 600,
        right: 800,
        bottom: 600,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;

    const hook = holder.current;
    assert(hook, "hook mounted");

    await act(async () => {
      hook.onNodeMouseDown(
        {
          stopPropagation() {},
          preventDefault() {},
          clientX: 200,
          clientY: 200,
        } as ReactMouseEvent,
        taskNode!,
      );
    });

    const xAfterDown = holder.current?.dragOverride?.x;

    await act(async () => {
      window.dispatchEvent(new window.MouseEvent("mousemove", { clientX: 220, clientY: 210 }));
      window.dispatchEvent(new window.MouseEvent("mousemove", { clientX: 260, clientY: 230 }));
    });

    assert(holder.current?.dragOverride?.x === xAfterDown, "override unchanged before RAF flush");

    await act(async () => {
      (globalThis as unknown as { __flushRaf: () => void }).__flushRaf();
    });

    assert(holder.current?.dragOverride?.x !== xAfterDown, "override updated after RAF flush");

    await act(async () => {
      window.dispatchEvent(new window.MouseEvent("mouseup"));
      root.unmount();
    });
  });

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
  console.log(`\n✓ ${passed}/${tests.length} useCanvasNodeDrag tests passed`);
}

void main();
