// Run: npx tsx src/components/Canvas/PlansProvider.test.tsx

import { act, useEffect, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { PlanButton } from "./PlanButton";
import {
  PlansProvider,
  useCanvasPlans,
  useFullPlans,
  useSidebarPlansOptional,
} from "./PlansProvider";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const API: ApiConfig = {
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: true,
};

let indexCalls = 0;
let fullCalls = 0;
let fullFixturePlans = [FIXTURE_PLAN];
let lastFullProject: string | null = null;
type ListenHandler = (ev: { payload: unknown }) => void;
const listenHandlers = new Map<string, Set<ListenHandler>>();

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/plans-provider" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/api/plans?") && url.includes("full=1")) {
        fullCalls++;
        return new Response(JSON.stringify({ plans: [FIXTURE_PLAN] }), { status: 200 });
      }
      if (url.endsWith("/api/plans")) {
        indexCalls++;
        const entry = {
          plan_id: FIXTURE_PLAN.id,
          file_path: "/tmp/plan.json",
          owner: FIXTURE_PLAN.owner,
          title: FIXTURE_PLAN.title,
          status: FIXTURE_PLAN.status,
          updated_at: FIXTURE_PLAN.updated_at,
          assigned_tab_ids: [],
        };
        return new Response(JSON.stringify({ plans: [entry] }), { status: 200 });
      }
      if (url.includes(`/api/plans/${FIXTURE_PLAN.id}`)) {
        return new Response(JSON.stringify(FIXTURE_PLAN), { status: 200 });
      }
      return new Response("not found", { status: 404 });
    },
  });
}

async function installListenMock() {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async (command, args) => {
    if (command === "list_plans_full") {
      fullCalls++;
      lastFullProject = (args?.project as string | null | undefined) ?? null;
      return fullFixturePlans as never;
    }
    if (command === "list_plan_index") {
      indexCalls++;
      return [{
        plan_id: FIXTURE_PLAN.id,
        file_path: "/tmp/plan.json",
        owner: FIXTURE_PLAN.owner,
        title: FIXTURE_PLAN.title,
        status: FIXTURE_PLAN.status,
        updated_at: FIXTURE_PLAN.updated_at,
        assigned_tab_ids: [],
      }] as never;
    }
    if (command === "get_plan_full") return FIXTURE_PLAN as never;
    throw new Error(`unexpected invoke: ${command}`);
  });
  __setListenForTest(async (event, handler) => {
    if (!listenHandlers.has(event)) listenHandlers.set(event, new Set());
    listenHandlers.get(event)!.add(handler as ListenHandler);
    return () => {
      listenHandlers.get(event)?.delete(handler as ListenHandler);
    };
  });
}

function emitPlansLoaded() {
  for (const handler of listenHandlers.get("plans-loaded") ?? []) {
    handler({ payload: {} });
  }
}

function FullProbe() {
  useFullPlans();
  return null;
}

function SidebarProbe() {
  const { plans } = useSidebarPlansOptional();
  return <output data-testid="sidebar-plan-count">{plans.length}</output>;
}

async function flushAsyncWork() {
  await Promise.resolve();
  await Promise.resolve();
}

async function renderTree(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await flushAsyncWork();
  });
  return { host, root };
}

test("provider fetches one index and one full snapshot regardless of consumer count", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { host, root } = await renderTree(
    <PlansProvider api={API}>
      <PlanButton tabId="orch-1" tabs={{}} />
      <PlanButton tabId="orch-1" tabs={{}} />
      <PlanButton tabId="orch-1" tabs={{}} />
      <PlanButton tabId="orch-1" tabs={{}} />
      <PlanButton tabId="orch-1" tabs={{}} />
    </PlansProvider>,
  );
  const buttons = host.querySelectorAll('[data-testid="plan-button"]');
  assert(buttons.length === 5, "five buttons rendered");
  assert(indexCalls === 1, `one index fetch for N consumers (got ${indexCalls})`);
  assert(fullCalls === 1, `one shared full fetch (got ${fullCalls})`);
  await act(async () => root.unmount());
});

test("plans-loaded event refetches the sidebar plan index immediately", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { root } = await renderTree(
    <PlansProvider api={API}>
      <PlanButton tabId="orch-1" tabs={{}} />
    </PlansProvider>,
  );
  assert(indexCalls === 1, `one index fetch before event (got ${indexCalls})`);
  await act(async () => {
    emitPlansLoaded();
    await flushAsyncWork();
  });
  const callsAfterEvent: number = indexCalls;
  assert(callsAfterEvent === 2, `plans-loaded refetches index (got ${indexCalls})`);
  await act(async () => root.unmount());
});

test("full bodies fetched once only when a canvas consumer mounts", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { root } = await renderTree(
    <PlansProvider api={API}>
      <PlanButton tabId="orch-1" tabs={{}} />
      <FullProbe />
      <FullProbe />
    </PlansProvider>,
  );
  assert(indexCalls === 1, `one index fetch (got ${indexCalls})`);
  assert(fullCalls === 1, `one batched full fetch for N canvas consumers (got ${fullCalls})`);
  await act(async () => root.unmount());
});

test("sidebar owns full plans without mounting canvas", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { host, root } = await renderTree(
    <PlansProvider api={API}>
      <SidebarProbe />
    </PlansProvider>,
  );
  assert(fullCalls === 1, `sidebar triggers one full fetch (got ${fullCalls})`);
  assert(
    host.querySelector('[data-testid="sidebar-plan-count"]')?.textContent === "1",
    "sidebar receives the full plan body",
  );
  await act(async () => root.unmount());
});

test("desktop plan queries wait for config and bypass REST", async () => {
  indexCalls = 0;
  fullCalls = 0;
  let restCalls = 0;
  const prevFetch = globalThis.fetch;
  globalThis.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    restCalls++;
    return prevFetch(input, init);
  };
  const unauthenticatedApi = { ...API, token: null };
  const { host, root } = await renderTree(
    <PlansProvider api={unauthenticatedApi} enabled={false}>
      <SidebarProbe />
    </PlansProvider>,
  );
  assert(fullCalls === 0, "disabled provider makes no unauthenticated full request");

  await act(async () => {
    root.render(
      <PlansProvider api={API} enabled>
        <SidebarProbe />
      </PlansProvider>,
    );
    await flushAsyncWork();
  });
  const callsAfterEnable: number = fullCalls;
  assert(callsAfterEnable === 1, "loaded config enables one full IPC read");
  assert(restCalls === 0, "desktop Plan reads never cross the REST auth boundary");
  assert(
    host.querySelector('[data-testid="sidebar-plan-count"]')?.textContent === "1",
    "sidebar receives plans after authentication",
  );
  globalThis.fetch = prevFetch;
  await act(async () => root.unmount());
});

test("canvas release cannot drop plans while sidebar remains mounted", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { host, root } = await renderTree(
    <PlansProvider api={API}>
      <SidebarProbe />
      <FullProbe />
    </PlansProvider>,
  );
  assert(fullCalls === 1, `shared consumers trigger one full fetch (got ${fullCalls})`);

  await act(async () => {
    root.render(
      <PlansProvider api={API}>
        <SidebarProbe />
      </PlansProvider>,
    );
    await flushAsyncWork();
  });
  await act(async () => {
    emitPlansLoaded();
    await flushAsyncWork();
  });

  const callsAfterCanvasRelease: number = fullCalls;
  assert(
    callsAfterCanvasRelease === 2,
    `sidebar refetches after canvas release (got ${fullCalls})`,
  );
  assert(
    host.querySelector('[data-testid="sidebar-plan-count"]')?.textContent === "1",
    "sidebar retains the full plan body after canvas release",
  );
  await act(async () => root.unmount());
});

test("workspace canvas does not rescope the shared full-plan query", async () => {
  indexCalls = 0;
  fullCalls = 0;
  lastFullProject = "unset";

  const { root } = await renderTree(
    <PlansProvider api={API}>
      <WorkspaceProbe projectPath="/repo/a" />
    </PlansProvider>,
  );
  assert(fullCalls === 1, "one scoped full fetch");
  assert(lastFullProject === null, "shared IPC query remains global while canvas filters locally");
  await act(async () => root.unmount());
});

test("two projects: workspace canvas consumer only loads matching project plans", async () => {
  const planA = { ...FIXTURE_PLAN, id: "plan-a", project_path: "/repo/a", title: "Plan A" };
  const planB = { ...FIXTURE_PLAN, id: "plan-b", project_path: "/repo/b", title: "Plan B" };
  fullFixturePlans = [planA, planB];

  const seen: string[] = [];
  function CapturePlans() {
    const { plans } = useCanvasPlans("workspace", "/repo/a");
    useEffect(() => {
      seen.push(...plans.map((p) => p.id));
    }, [plans]);
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <PlansProvider api={API}>
        <CapturePlans />
      </PlansProvider>,
    );
  });
  await act(async () => {
    await flushAsyncWork();
  });
  assert(seen.includes("plan-a"), "plan A loaded for project /repo/a");
  assert(!seen.includes("plan-b"), "plan B excluded for project /repo/a");
  fullFixturePlans = [FIXTURE_PLAN];
  await act(async () => root.unmount());
});

function WorkspaceProbe({ projectPath }: { projectPath: string }) {
  useCanvasPlans("workspace", projectPath);
  return null;
}

async function main() {
  setupDom();
  await installListenMock();
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
  console.log(`\n✓ ${passed}/${tests.length} PlansProvider tests passed`);
}

main();
