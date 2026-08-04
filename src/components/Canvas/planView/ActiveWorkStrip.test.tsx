// Run: npx tsx src/components/Canvas/planView/ActiveWorkStrip.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../cards/fixtures.testdata";
import { ActiveWorkStrip } from "./ActiveWorkStrip";
import { selectActiveWork } from "./activeWork";
import { TaskStatus } from "../types";

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
  const window = new Window({ url: "http://localhost/active-work-strip" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
}

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => { root.render(ui); });
  return {
    host,
    unmount: async () => {
      await act(async () => { root.unmount(); });
      host.remove();
    },
  };
}

test("renders nothing when no in-flight tasks", async () => {
  const { host, unmount } = await render(<ActiveWorkStrip entries={[]} onOpenTask={() => {}} />);
  assert(!host.querySelector('[data-testid="active-work-strip"]'), "empty");
  await unmount();
});

test("renders strip rows with stable test ids", async () => {
  const plan = {
    ...FIXTURE_PLAN,
    tasks: [
      { ...FIXTURE_TASK, id: "t1", status: TaskStatus.InProgress, assigned_tab_id: "p1", updated_at: 100 },
    ],
  };
  const snapshot = selectActiveWork([plan], { p1: { id: "p1", pty_id: null, tab_type: "Terminal", title: "Worker", cwd: null } as never }, 200);
  let opened = "";
  const { host, unmount } = await render(
    <ActiveWorkStrip entries={snapshot.entries} onOpenTask={(e) => { opened = e.task.id; }} />,
  );
  assert(host.querySelector('[data-testid="active-work-strip"]'), "strip root");
  const row = host.querySelector('[data-testid="active-work-strip-row"]');
  assert(row, "row");
  assert(row?.getAttribute("data-task-id") === "t1", "task id");
  await act(async () => { (row as HTMLButtonElement).click(); });
  assert(opened === "t1", "click opens task");
  await unmount();
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
