// Run: npx tsx src/components/Canvas/panePlanMode.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { isPanePlanMode, setPanePlanMode, usePanePlanMode } from "./panePlanMode";

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
  const window = new Window({ url: "http://localhost/plan-mode" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const key of ["window", "document", "navigator", "HTMLElement", "Node", "Event"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as unknown as Record<string, unknown>)[key], configurable: true });
  }
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

function Probe({ paneId }: { paneId: string }) {
  const active = usePanePlanMode(paneId);
  return <span data-testid="state">{active ? "plan" : "off"}</span>;
}

test("selecting Plan activates the third mode for that pane only", async () => {
  setupDom();
  const { host, root } = await render(
    <>
      <Probe paneId="pane-a" />
    </>,
  );
  const cell = () => host.querySelector('[data-testid="state"]')?.textContent;
  assert(cell() === "off", "starts off");

  await act(async () => setPanePlanMode("pane-a", true));
  assert(cell() === "plan", "Plan mode activates");
  assert(isPanePlanMode("pane-a"), "store reflects active");
  assert(!isPanePlanMode("pane-b"), "other panes unaffected (mutually exclusive per pane)");

  await act(async () => setPanePlanMode("pane-a", false));
  assert(cell() === "off", "Chat/Terminal returns clears Plan mode");
  await act(async () => root.unmount());
});

test("idempotent set does not thrash subscribers", async () => {
  let renders = 0;
  function Counter({ paneId }: { paneId: string }) {
    usePanePlanMode(paneId);
    renders++;
    return null;
  }
  const { root } = await render(<Counter paneId="pane-c" />);
  const before = renders;
  await act(async () => setPanePlanMode("pane-c", false)); // already off → no notify
  assert(renders === before, "no re-render on no-op set");
  await act(async () => setPanePlanMode("pane-c", true));
  assert(renders > before, "re-render on real change");
  await act(async () => root.unmount());
});

async function main() {
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
  console.log(`\n✓ ${passed}/${tests.length} panePlanMode tests passed`);
}

main();
