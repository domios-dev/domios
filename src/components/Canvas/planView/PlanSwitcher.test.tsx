// Run: npx tsx src/components/Canvas/planView/PlanSwitcher.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PlanSwitcher, type PlanSwitcherOption } from "./PlanSwitcher";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const window = new Window({ url: "http://localhost/plan-switcher" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function opts(n: number): PlanSwitcherOption[] {
  return Array.from({ length: n }, (_, i) => ({ id: `p-${i}`, title: `Plan ${i}` }));
}

test("renders nothing for a single plan", async () => {
  const { host } = await render(<PlanSwitcher plans={opts(1)} selectedId="p-0" onSelect={() => {}} />);
  assert(!host.querySelector('[data-testid="plan-switcher"]'), "no switcher");
});

test("segmented buttons for <=3 plans; click selects", async () => {
  let picked = "";
  const { host } = await render(
    <PlanSwitcher plans={opts(3)} selectedId="p-0" onSelect={(id) => (picked = id)} />,
  );
  const options = host.querySelectorAll('[data-testid="plan-switcher-option"]');
  assert(options.length === 3, `segmented count=${options.length}`);
  assert(!host.querySelector('[data-testid="plan-switcher-select"]'), "no dropdown");
  const active = host.querySelector('[data-testid="plan-switcher-option"][data-plan-id="p-0"]');
  assert(active?.getAttribute("data-active") === "true", "selected marked active");
  const target = host.querySelector('[data-testid="plan-switcher-option"][data-plan-id="p-2"]') as HTMLElement;
  await act(async () => {
    target.click();
  });
  assert(picked === "p-2", `selected=${picked}`);
});

test("dropdown for >3 plans; change selects", async () => {
  let picked = "";
  const { host } = await render(
    <PlanSwitcher plans={opts(5)} selectedId="p-1" onSelect={(id) => (picked = id)} />,
  );
  const select = host.querySelector('[data-testid="plan-switcher-select"]') as HTMLSelectElement;
  assert(select, "dropdown present");
  assert(!host.querySelector('[data-testid="plan-switcher-option"]'), "no segmented buttons");
  assert(select.value === "p-1", "selected value bound");
  await act(async () => {
    select.value = "p-4";
    select.dispatchEvent(new window.Event("change", { bubbles: true }) as unknown as Event);
  });
  assert(picked === "p-4", `selected=${picked}`);
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
  console.log(`\n✓ ${passed}/${tests.length} PlanSwitcher tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
