import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setListenForTest } from "../../lib/bridge";
import { configureDaemonTransport } from "../../lib/daemonBootstrap";
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
  Object.assign(window, { __CODETERM_DAEMON__: { origin: /^https?:/.test(window.location.origin) ? window.location.origin : "http://127.0.0.1:7686", token: "test-token" } });
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
  dispatchPlanChanged: (planId?: string) => Promise<void>;
  chipStates: () => string[];
  teardown: () => Promise<void>;
}

async function mountResolver(responses: ResolvedBody[], additionalRefs: PlanRefDescriptor[] = []): Promise<Harness> {
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
  const pending: Array<(response: Response) => void> = [];
  const client = configureDaemonTransport({ origin: window.location.origin, token: "test-token" });
  const originalRequest = client.request;
  client.request = (path, init) => {
    assert(path === "/api/plans/resolve-refs", `unexpected request: ${path}`);
    fetchCalls.push(String(init?.body ?? ""));
    return new Promise(resolve => pending.push(resolve));
  };
  const respond = async () => act(() => {
    const deliver = pending.shift();
    const body = queue.shift();
    assert(deliver && body, "expected one pending reference request and response");
    deliver(Response.json(body));
  });

  const api = { enabled: true, host: "127.0.0.1", port: 7686, token: null, cors: false };
  const refs: PlanRefDescriptor[] = [{ planId: "pl_test", targetId: "task_test", targetKind: "task" }, ...additionalRefs];
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
  assert(snapshots.length > 0 && snapshots.every(item => item.state == null), "the chip stays loading until its first response");
  await respond();

  return {
    fetchCalls,
    snapshots,
    dispatchPlanChanged: async (planId = "pl_test") => {
      await act(async () => {
        const handlers = listeners.get("plan-changed");
        assert(handlers?.length, "plan-changed subscription is installed");
        for (const handler of handlers) handler({ payload: { plan_id: planId } });
      });
      if (pending.length) await respond();
    },
    chipStates: () => snapshots
      .map((s) => (s.state == null ? "loading" : `${s.state}:${s.title}`))
      .filter((state, index, all) => index === 0 || state !== all[index - 1]),
    teardown: async () => {
      await act(async () => {
        root.unmount();
      });
      host.remove();
      assert(pending.length === 0 && queue.length === 0, "all reference responses were consumed");
      client.request = originalRequest;
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

test("a plan change revalidates only that plan and ignores unrelated changes", async () => {
  const other = { ...refBody("Other").refs[0], plan_id: "pl_other", target_id: "task_other" };
  const harness = await mountResolver([{ refs: [...refBody("First").refs, other] }, refBody("Renamed")],
    [{ planId: "pl_other", targetId: "task_other", targetKind: "task" }]);
  await harness.dispatchPlanChanged("pl_unrelated");
  assert(harness.fetchCalls.length === 1, "unrelated plans do not refetch visible refs");
  await harness.dispatchPlanChanged();
  const sent = JSON.parse(harness.fetchCalls[1]).refs;
  assert(sent.length === 1 && sent[0].plan_id === "pl_test", "only the changed plan is revalidated");
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
