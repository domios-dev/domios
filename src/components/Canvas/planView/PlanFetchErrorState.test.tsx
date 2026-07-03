// Run: npx tsx src/components/Canvas/planView/PlanFetchErrorState.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./PlanFetchErrorState";

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
  const window = new Window({ url: "http://localhost/plan-fetch-error" });
  for (const k of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "Node"] as const) {
    Object.defineProperty(globalThis, k, { value: (window as never)[k], configurable: true });
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

test("never renders raw transport error text", async () => {
  const { host, root } = await render(
    <PlanFetchErrorState
      error="Could not reach CodeTerm. Reconnecting…"
      testId="plan-board-error"
    />,
  );
  const el = host.querySelector('[data-testid="plan-board-error"]');
  assert(el?.textContent?.includes("Reconnecting"), "friendly copy");
  assert(!el?.textContent?.includes("TypeError"), "no raw exception");
  await act(async () => root.unmount());
});

test("retry button calls onRetry", async () => {
  let retries = 0;
  const { host, root } = await render(
    <PlanFetchErrorState
      error="Could not reach CodeTerm. Reconnecting…"
      onRetry={() => {
        retries++;
      }}
      testId="plan-board-error"
    />,
  );
  const btn = host.querySelector('[data-testid="plan-board-error-retry"]') as HTMLButtonElement;
  await act(async () => {
    btn.click();
  });
  assert(retries === 1, "retry invoked");
  await act(async () => root.unmount());
});

test("reconnecting banner when stale data present", async () => {
  const { host, root } = await render(
    <PlanFetchReconnectingBanner reconnecting itemCount={2} testId="plan-board-reconnecting" />,
  );
  assert(host.querySelector('[data-testid="plan-board-reconnecting"]'), "reconnecting banner");
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
