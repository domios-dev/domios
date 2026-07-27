// Run: npx tsx src/components/Canvas/CanvasButton.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { canvasFocusStore } from "./canvasFocusStore";
import { CanvasButton } from "./CanvasButton";

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
  const window = new Window({ url: "http://127.0.0.1/canvas-button" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
}

async function renderButton(active: boolean): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<CanvasButton active={active} />);
  });
  return { host, root };
}

test("CanvasButton renders in toolbar", async () => {
  const { host, root } = await renderButton(false);
  const button = host.querySelector('[data-testid="canvas-toolbar-button"]');
  assert(button, "button present");
  assert(button.getAttribute("aria-label") === "Open workspace canvas", "button has an accessible action name");
  assert(button.getAttribute("aria-pressed") === "false", "closed canvas is not pressed");
  assert(button.className.includes("ct-liquid-interactive"), "tray uses shared interaction class");
  await act(async () => root.unmount());
});

test("CanvasButton toggles workspace canvas open state", async () => {
  canvasFocusStore.closeWorkspace();
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<CanvasButton active={false} />);
  });
  const btn = () => host.querySelector('[data-testid="canvas-toolbar-button"]') as HTMLButtonElement;
  await act(async () => btn().click());
  assert(canvasFocusStore.isWorkspaceOpen(), "workspace canvas open after click");
  await act(async () => {
    root.render(<CanvasButton active={true} />);
  });
  assert(btn().getAttribute("aria-label") === "Close workspace canvas", "open canvas exposes close action");
  assert(btn().getAttribute("aria-pressed") === "true", "open canvas is pressed");
  await act(async () => btn().click());
  assert(canvasFocusStore.isWorkspaceDismissed(), "workspace canvas dismissed after second click");
  canvasFocusStore.closeWorkspace();
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
  console.log(`\n✓ ${passed}/${tests.length} CanvasButton tests passed`);
}

main();
