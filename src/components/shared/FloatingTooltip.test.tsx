// Run: npx tsx src/components/shared/FloatingTooltip.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FloatingTooltip } from "./FloatingTooltip";

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
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  Object.defineProperty(globalThis, "PointerEvent", {
    value: window.PointerEvent ?? window.MouseEvent,
    configurable: true,
  });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    // Deterministic sync RAF for tests — no dependency on real frame timing.
    value: (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    },
    configurable: true,
  });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

setupDom();

test("hovering the anchor shows the tooltip with zero added delay (no timers involved)", async () => {
  const { host, root } = await render(
    <FloatingTooltip content={<div data-testid="tip-body">Hello</div>}>
      <span data-testid="anchor">?</span>
    </FloatingTooltip>,
  );
  const anchor = host.querySelector('[data-testid="anchor"]') as HTMLElement;
  assert(anchor, "anchor renders");
  assert(!document.body.querySelector('[data-testid="tip-body"]'), "tooltip hidden before hover");

  await act(async () => {
    const event = new (globalThis as unknown as { PointerEvent: typeof MouseEvent }).PointerEvent("pointerover", {
      bubbles: true,
    }) as PointerEvent;
    Object.defineProperty(event, "pointerType", { value: "mouse" });
    anchor.dispatchEvent(event);
  });

  assert(document.body.querySelector('[data-testid="tip-body"]'), "tooltip visible immediately on hover");
  const tooltip = document.body.querySelector('[role="tooltip"]');
  assert(tooltip?.id, "tooltip has a stable accessible id");
  assert(anchor.parentElement?.getAttribute("aria-describedby") === tooltip.id, "anchor references the visible tooltip");

  await act(async () => root.unmount());
});

test("a disabled tooltip never shows on hover", async () => {
  const { host, root } = await render(
    <FloatingTooltip content={<div data-testid="tip-body-2">Hello</div>} disabled>
      <span data-testid="anchor-2">?</span>
    </FloatingTooltip>,
  );
  const anchor = host.querySelector('[data-testid="anchor-2"]') as HTMLElement;

  await act(async () => {
    const event = new (globalThis as unknown as { PointerEvent: typeof MouseEvent }).PointerEvent("pointerover", {
      bubbles: true,
    }) as PointerEvent;
    Object.defineProperty(event, "pointerType", { value: "mouse" });
    anchor.dispatchEvent(event);
  });

  assert(!document.body.querySelector('[data-testid="tip-body-2"]'), "disabled tooltip stays hidden");
  await act(async () => root.unmount());
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  console.log(`FloatingTooltip tests: ${tests.length - failed}/${tests.length} passed`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
