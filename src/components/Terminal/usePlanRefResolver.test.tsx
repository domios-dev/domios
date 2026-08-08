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

interface ResolvedBody {
  refs: Array<{
    plan_id: string;
    target_id: string;
    target_kind: string;
    state: string;
    title: string;
    plan_title: string;
  }>;
}

function refBody(title: string, state = "resolved"): ResolvedBody {
  return {
    refs: [{
      plan_id: "pl_test",
      target_id: "task_test",
      target_kind: "task",
      state,
      title,
      plan_title: "Plan",
    }],
  };
}

interface Harness {
  fetchCalls: string[];
  snapshots: Array<{ title?: string; state?: string }>;
  dispatchPlanChanged: () => Promise<void>;
  chipStates: () => string[];
  teardown: () => Promise<void>;
}

/** Mounts one chip-shaped probe over the hook; a missing map entry is exactly
 *  what TabSessionChat renders as the "loading" chip state. */
async function mountResolver(responses: ResolvedBody[]): Promise<Harness> {
  setupDom();
  const listeners = new Map<string, Array<(e: { payload: unknown }) => void>>();
  __setListenForTest(async (event, handler) => {
    const arr = listeners.get(event) ?? [];
    arr.push(handler as (e: { payload: unknown }) => void);
    listeners.set(event, arr);
    return () => {};
  });

  const fetchCalls: string[] = [];
  const queue = [...responses];
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (_url: string, init?: RequestInit) => {
      fetchCalls.push(String(init?.body ?? ""));
      const body = queue.shift() ?? { refs: [] };
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

  return {
    fetchCalls,
    snapshots,
    dispatchPlanChanged: async () => {
      await act(async () => {
        for (const handler of listeners.get("plan-changed") ?? []) handler({ payload: { plan_id: "pl_test" } });
        await Promise.resolve();
      });
      await act(async () => {
        await Promise.resolve();
      });
    },
    // Consecutive-collapsed chip states: the sequence of distinct views the
    // chip actually transitioned through, "loading" when unresolved.
    chipStates: () => snapshots
      .map((s) => (s.state == null ? "loading" : `${s.state}:${s.title}`))
      .filter((state, index, all) => index === 0 || state !== all[index - 1]),
    teardown: async () => {
      await act(async () => {
        root.unmount();
      });
      host.remove();
      __setListenForTest(null);
    },
  };
}

test("a never-resolved ref surfaces loading once, then resolves on first fetch", async () => {
  const harness = await mountResolver([refBody("First title")]);
  assert(harness.fetchCalls.length === 1, `expected one initial fetch, got ${harness.fetchCalls.length}`);
  const states = harness.chipStates();
  assert(
    JSON.stringify(states) === JSON.stringify(["loading", "resolved:First title"]),
    `first resolution transitioned through ${JSON.stringify(states)}`,
  );
  await harness.teardown();
});

test("plan-changed with unchanged data revalidates with zero transitions on a resolved chip", async () => {
  const harness = await mountResolver([refBody("Stable title"), refBody("Stable title")]);
  await harness.dispatchPlanChanged();
  assert(harness.fetchCalls.length === 2, `expected a background refetch, got ${harness.fetchCalls.length} fetches`);
  const states = harness.chipStates();
  assert(
    JSON.stringify(states) === JSON.stringify(["loading", "resolved:Stable title"]),
    `an unchanged revalidation transitioned the chip: ${JSON.stringify(states)}`,
  );
  await harness.teardown();
});

test("plan-changed with a renamed task updates the chip without an intermediate loading state", async () => {
  const harness = await mountResolver([refBody("Old title"), refBody("New title")]);
  await harness.dispatchPlanChanged();
  assert(harness.fetchCalls.length === 2, `expected a background refetch, got ${harness.fetchCalls.length} fetches`);
  const states = harness.chipStates();
  assert(
    JSON.stringify(states) === JSON.stringify(["loading", "resolved:Old title", "resolved:New title"]),
    `a rename transitioned the chip through ${JSON.stringify(states)}`,
  );
  assert(
    !states.slice(1).includes("loading"),
    "a resolved chip passed back through loading during revalidation",
  );
  await harness.teardown();
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
