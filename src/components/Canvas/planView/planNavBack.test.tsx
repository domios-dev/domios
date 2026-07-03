// Run: npx tsx src/components/Canvas/planView/planNavBack.test.tsx
// Focused tests: back button visibility and keyboard in PlanDetailOverlay.

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { PlanDetailOverlay } from "./PlanDetailOverlay";

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
  const window = new Window({ url: "http://localhost/plan-nav-back" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "KeyboardEvent", { value: window.KeyboardEvent, configurable: true });
}

setupDom();

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  host.style.width = "1200px";
  host.style.height = "800px";
  host.style.position = "relative";
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(ui); });
  await act(async () => { await Promise.resolve(); });
  return { host, root };
}

async function cleanup(root: Root, host: HTMLDivElement) {
  await act(async () => { root.unmount(); });
  host.remove();
}

test("back button absent when canGoBack is false (default)", async () => {
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} onClose={() => {}} />,
  );
  assert(!host.querySelector('[data-testid="plan-detail-back"]'), "no back button when canGoBack not set");
  await cleanup(root, host);
});

test("back button absent when canGoBack is explicitly false", async () => {
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} canGoBack={false} onBack={() => {}} onClose={() => {}} />,
  );
  assert(!host.querySelector('[data-testid="plan-detail-back"]'), "no back button when canGoBack=false");
  await cleanup(root, host);
});

test("back button visible when canGoBack is true", async () => {
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} canGoBack onBack={() => {}} onClose={() => {}} />,
  );
  const btn = host.querySelector('[data-testid="plan-detail-back"]') as HTMLButtonElement | null;
  assert(btn !== null, "back button exists");
  assert(btn.getAttribute("aria-label") === "Go back", "aria-label");
  await cleanup(root, host);
});

test("clicking back button calls onBack", async () => {
  let called = false;
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} canGoBack onBack={() => { called = true; }} onClose={() => {}} />,
  );
  const btn = host.querySelector('[data-testid="plan-detail-back"]') as HTMLButtonElement;
  await act(async () => { btn.click(); });
  assert(called, "onBack called on button click");
  await cleanup(root, host);
});

test("Alt+Left triggers onBack when canGoBack", async () => {
  let backCalled = false;
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} canGoBack onBack={() => { backCalled = true; }} onClose={() => {}} />,
  );
  const overlay = host.querySelector('[data-testid="plan-detail-overlay"]') as HTMLElement;
  await act(async () => {
    overlay.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", altKey: true, bubbles: true, cancelable: true }));
  });
  assert(backCalled, "onBack called on Alt+ArrowLeft");
  await cleanup(root, host);
});

test("Alt+Left does NOT trigger onBack when canGoBack is false", async () => {
  let backCalled = false;
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} canGoBack={false} onBack={() => { backCalled = true; }} onClose={() => {}} />,
  );
  const overlay = host.querySelector('[data-testid="plan-detail-overlay"]') as HTMLElement;
  await act(async () => {
    overlay.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", altKey: true, bubbles: true, cancelable: true }));
  });
  assert(!backCalled, "onBack NOT called when canGoBack=false");
  await cleanup(root, host);
});

test("Escape still closes, not triggers back", async () => {
  let closed = false;
  let backed = false;
  const { host, root } = await render(
    <PlanDetailOverlay
      kind="task"
      task={FIXTURE_TASK}
      canGoBack
      onBack={() => { backed = true; }}
      onClose={() => { closed = true; }}
    />,
  );
  const overlay = host.querySelector('[data-testid="plan-detail-overlay"]') as HTMLElement;
  await act(async () => {
    overlay.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  });
  assert(closed, "Esc closes");
  assert(!backed, "Esc does not trigger back");
  await cleanup(root, host);
});

test("round-trip: back button appears after first jump, disappears after back", async () => {
  let backCount = 0;
  let canGoBack = true;

  const { host, root } = await render(
    <PlanDetailOverlay
      kind="plan"
      plan={FIXTURE_PLAN}
      canGoBack={canGoBack}
      onBack={() => { backCount++; canGoBack = false; }}
      onClose={() => {}}
    />,
  );
  assert(host.querySelector('[data-testid="plan-detail-back"]') !== null, "back button visible with canGoBack=true");

  const btn = host.querySelector('[data-testid="plan-detail-back"]') as HTMLButtonElement;
  await act(async () => { btn.click(); });
  assert(backCount === 1, "onBack called once");

  await act(async () => {
    root.render(
      <PlanDetailOverlay
        kind="plan"
        plan={FIXTURE_PLAN}
        canGoBack={false}
        onBack={() => { backCount++; }}
        onClose={() => {}}
      />,
    );
  });
  assert(host.querySelector('[data-testid="plan-detail-back"]') === null, "back button gone after pop empties stack");
  await cleanup(root, host);
});

test("scrollCaptureRef is assigned to body element", async () => {
  const captureRef: { current: HTMLDivElement | null } = { current: null };
  const { host, root } = await render(
    <PlanDetailOverlay kind="task" task={FIXTURE_TASK} scrollCaptureRef={captureRef} onClose={() => {}} />,
  );
  const body = host.querySelector('[data-testid="plan-detail-body"]') as HTMLDivElement;
  assert(captureRef.current !== null, "scrollCaptureRef assigned");
  assert(captureRef.current === body, "scrollCaptureRef points to body element");
  await cleanup(root, host);
});

let passed = 0;
(async () => {
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} planNavBack tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
