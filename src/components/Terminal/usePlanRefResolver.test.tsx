import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import { usePlanRefResolver, type PlanRefDescriptor } from "./usePlanRefResolver";

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
  const window = new Window({ url: "http://localhost/use-plan-ref-resolver" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "AbortController", { value: window.AbortController, configurable: true });
}

test("plan-changed invalidates cache and refetches unchanged refs", async () => {
  setupDom();
  const listeners = new Map<string, Array<(e: { payload: unknown }) => void>>();
  __setListenForTest(async (event, handler) => {
    const arr = listeners.get(event) ?? [];
    arr.push(handler as (e: { payload: unknown }) => void);
    listeners.set(event, arr);
    return () => {};
  });

  const responses = [
    {
      refs: [{
        plan_id: "pl_test",
        target_id: "task_test",
        target_kind: "task",
        state: "resolved",
        title: "Old title",
        plan_title: "Plan",
      }],
    },
    {
      refs: [{
        plan_id: "pl_test",
        target_id: "task_test",
        target_kind: "task",
        state: "stale",
        title: "New title",
        plan_title: "Plan",
      }],
    },
  ];
  const fetchCalls: string[] = [];
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (_url: string, init?: RequestInit) => {
      fetchCalls.push(String(init?.body ?? ""));
      const body = responses.shift() ?? { refs: [] };
      return {
        ok: true,
        json: async () => body,
      };
    },
  });

  const api = { enabled: true, host: "127.0.0.1", port: 7686, token: null, cors: false };
  const refs: PlanRefDescriptor[] = [{ planId: "pl_test", targetId: "task_test", targetKind: "task" }];
  const snapshots: Array<{ title?: string; state?: string }> = [];

  function Probe() {
    const resolved = usePlanRefResolver(api, refs);
    const item = resolved.refs.get("task:pl_test:task_test");
    snapshots.push({ title: item?.title, state: item?.state });
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<Probe />);
  });
  await act(async () => {
    await Promise.resolve();
  });
  const initialFetchCount = fetchCalls.length;
  assert(initialFetchCount === 1, `expected initial fetch, got ${initialFetchCount}`);
  assert(snapshots.some((s) => s.title === "Old title" && s.state === "resolved"), "first response applied");

  await act(async () => {
    for (const handler of listeners.get("plan-changed") ?? []) handler({ payload: { plan_id: "pl_test" } });
    await Promise.resolve();
  });
  const refetchCount = fetchCalls.length;
  assert(refetchCount === 2, `expected refetch after invalidation, got ${refetchCount}`);
  assert(snapshots.some((s) => s.title === "New title" && s.state === "stale"), "post-invalidation response applied");

  await act(async () => {
    root.unmount();
  });
  host.remove();
  __setListenForTest(null);
});

let passed = 0;
async function main() {
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
  console.log(`\n✓ ${passed}/${tests.length} usePlanRefResolver tests passed`);
}

void main();
