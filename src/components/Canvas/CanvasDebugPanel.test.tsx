// Run: npx tsx src/components/Canvas/CanvasDebugPanel.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { CanvasDebugPanel } from "./CanvasDebugPanel";
import { CANVAS_LAYOUT_SETTING_KEYS } from "./settings";
import { CanvasSettingsProvider, useCanvasSettings } from "./useCanvasSettings";

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
  const window = new Window({ url: "http://localhost/canvas-debug" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLInputElement", { value: window.HTMLInputElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLSelectElement", { value: window.HTMLSelectElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
  window.localStorage.clear();
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}


function SettingsProbe() {
  const { settings, setField } = useCanvasSettings();
  return (
    <>
      <div data-testid="probe-width">{settings.planWidth}</div>
      <div data-testid="probe-strategy">{settings.layoutStrategy}</div>
      <button
        type="button"
        data-testid="probe-set-card-width"
        onClick={() => setField("planWidth", 400)}
      >
        bump
      </button>
    </>
  );
}

test("debug panel renders one control per CanvasLayoutSettings field", async () => {
  window.localStorage.clear();
  const { host, root } = await render(
    <CanvasSettingsProvider>
      <CanvasDebugPanel forceVisible />
    </CanvasSettingsProvider>,
  );

  const toggle = host.querySelector('[data-testid="canvas-debug-panel-toggle"]') as HTMLButtonElement;
  assert(toggle, "toggle button present");
  await act(async () => {
    toggle.click();
  });

  for (const key of CANVAS_LAYOUT_SETTING_KEYS) {
    assert(
      host.querySelector(`[data-testid="canvas-debug-field-${key}"]`),
      `missing control for ${key}`,
    );
  }

  await act(async () => root.unmount());
});

test("editing a field updates settings override via context", async () => {
  window.localStorage.clear();
  const { host, root } = await render(
    <CanvasSettingsProvider>
      <SettingsProbe />
      <CanvasDebugPanel forceVisible />
    </CanvasSettingsProvider>,
  );

  await act(async () => {
    (host.querySelector('[data-testid="canvas-debug-panel-toggle"]') as HTMLButtonElement).click();
  });

  const select = host.querySelector(
    '[data-testid="canvas-debug-input-layoutStrategy"]',
  ) as HTMLSelectElement;
  assert(select, "layoutStrategy select present");

  await act(async () => {
    select.value = "cluster";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });

  assert(
    host.querySelector('[data-testid="probe-strategy"]')?.textContent === "cluster",
    "panel edit updates merged settings",
  );

  await act(async () => {
    (host.querySelector('[data-testid="probe-set-card-width"]') as HTMLButtonElement).click();
  });
  assert(host.querySelector('[data-testid="probe-width"]')?.textContent === "400", "setField updates override");

  await act(async () => root.unmount());
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (e) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.message : e}`);
    }
  }
  console.log(`\n${tests.length - failed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

void main();
