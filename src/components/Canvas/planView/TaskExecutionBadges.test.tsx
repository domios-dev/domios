// Run: npx tsx src/components/Canvas/planView/TaskExecutionBadges.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { TaskExecutionBadges } from "./TaskExecutionBadges";

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
  const window = new Window({ url: "http://localhost/task-execution-badges" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

setupDom();

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

test("renders nothing when no badges apply", async () => {
  const { host } = await render(<TaskExecutionBadges />);
  assert(!host.querySelector('[data-testid="task-loop-badge"]'), "no loop badge");
  assert(!host.querySelector('[data-testid="task-compete-badge"]'), "no compete badge");
});

test("renders loop badge with warning tint at threshold", async () => {
  const { host } = await render(
    <TaskExecutionBadges
      loop={{
        count: 5,
        budget: 5,
        status: "running",
        stopWhen: "tests green",
        warning: true,
        tooltip: "iteration 5/5",
        loops: [],
      }}
    />,
  );
  const badge = host.querySelector('[data-testid="task-loop-badge"]');
  assert(badge, "loop badge visible");
  assert(badge?.textContent?.includes("×5"), "loop count");
  assert(badge?.getAttribute("title") === "iteration 5/5", "tooltip");
});

test("renders compete badge alongside loop badge", async () => {
  const { host } = await render(
    <TaskExecutionBadges
      loop={{
        count: 2,
        budget: 8,
        status: "running",
        stopWhen: null,
        warning: false,
        tooltip: "loop",
        loops: [],
      }}
      competition={{ count: 3, liveCount: 2, tooltip: "compete" }}
    />,
  );
  assert(host.querySelector('[data-testid="task-loop-badge"]'), "loop badge");
  const compete = host.querySelector('[data-testid="task-compete-badge"]');
  assert(compete, "compete badge");
  assert(compete?.textContent?.includes("×3"), "compete count");
  assert(compete?.getAttribute("title") === "compete", "compete tooltip");
});

let passed = 0;
(async () => {
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed += 1;
      console.log(`  ✓ ${name}`);
    } catch (error) {
      console.error(`  ✗ ${name}`);
      console.error(error);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} TaskExecutionBadges tests passed`);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
