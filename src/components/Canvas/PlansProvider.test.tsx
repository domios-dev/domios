import { muxMockFetch, settleDaemon } from "../../lib/muxMockFetch.testdata";
// Run: npx tsx src/components/Canvas/PlansProvider.test.tsx

import { act, useEffect, type ComponentProps, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig, Workspace } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { PlanButton as ActivityPlanButton } from "./PlanButton";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import {
  PlansProvider,
  useCanvasPlans,
  usePlanSummaries,
  usePlanSummariesOptional,
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
let chatCalls = 0;
const readIndexCalls = (): number => indexCalls;
const readFullCalls = (): number => fullCalls;
let fullFixturePlans = [FIXTURE_PLAN];
let lastFullProject: string | null = null;
type ListenHandler = (ev: { payload: unknown }) => void;
const listenHandlers = new Map<string, Set<ListenHandler>>();

const muxState: ExtendedState = {
  workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
  notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0,
  pendingCloses: [],
};
const muxStore = createMuxStateStore(() => muxState);

function PlanButton(props: ComponentProps<typeof ActivityPlanButton>) {
  return (
    <MuxStateStoreContext.Provider value={muxStore}>
      <ActivityPlanButton {...props} />
    </MuxStateStoreContext.Provider>
  );
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/plans-provider" });
  Object.assign(window, { __CODETERM_DAEMON__: { origin: /^https?:/.test(window.location.origin) ? window.location.origin : "http://127.0.0.1:7686", token: "test-token" } });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "HTMLButtonElement", { value: window.HTMLButtonElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: muxMockFetch(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/api/plans?") && url.includes("view=summary")) {
        fullCalls++;
        lastFullProject = new URL(url).searchParams.get("project");
        return new Response(JSON.stringify({ plans: fullFixturePlans.map((plan) => ({ ...plan, revision: "1" })) }), { status: 200 });
      }
      if (url.includes("chat=1")) { chatCalls++; return Response.json({ plans: fullFixturePlans.map((plan) => ({ ...plan, revision: "1" })) }); }
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
    }),
  });
}

async function installListenMock() {
  const { __setListenForTest } = await import("../../lib/bridge");
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
  usePlanSummaries();
  return null;
}

function OptionalFullProbe({ enabled }: { enabled: boolean }) {
  const full = usePlanSummariesOptional(enabled);
  return <output data-testid="optional-full-state">{full ? "enabled" : "disabled"}</output>;
}

function SidebarProbe() {
  const { plans } = useSidebarPlansOptional();
  return <output data-testid="sidebar-plan-count">{plans.length}</output>;
}

// Carrier responses arrive as mux frames; settle on the carrier's own idle acknowledgement.
async function flushAsyncWork() {
  await settleDaemon();
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
  assert(readIndexCalls() === 1, `one index fetch for N consumers (got ${readIndexCalls()})`);
  assert(readFullCalls() === 0, "chat controls never download full plans");
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
  assert(readIndexCalls() === 1, `one index fetch before event (got ${readIndexCalls()})`);
  await act(async () => {
    emitPlansLoaded();
    await flushAsyncWork();
  });
  const callsAfterEvent: number = readIndexCalls();
  assert(callsAfterEvent === 2, `plans-loaded refetches index (got ${readIndexCalls()})`);
  await act(async () => root.unmount());
});

test("summaries fetched once only when a canvas consumer mounts", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { root } = await renderTree(
    <PlansProvider api={API}>
      <PlanButton tabId="orch-1" tabs={{}} />
      <FullProbe />
      <FullProbe />
    </PlansProvider>,
  );
  assert(readIndexCalls() === 1, `one index fetch (got ${readIndexCalls()})`);
  assert(readFullCalls() === 1, `one batched full fetch for N canvas consumers (got ${readFullCalls()})`);
  await act(async () => root.unmount());
});

test("optional summary reader leaves Tasks detached and releases the collection when hidden", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { host, root } = await renderTree(
    <PlansProvider api={API}>
      <OptionalFullProbe enabled={false} />
    </PlansProvider>,
  );
  assert(readFullCalls() === 0, "disabled Tasks mode does not subscribe to the full collection");
  assert(host.querySelector('[data-testid="optional-full-state"]')?.textContent === "disabled", "no full snapshot exposed");

  await act(async () => {
    root.render(
      <PlansProvider api={API}>
        <OptionalFullProbe enabled />
      </PlansProvider>,
    );
    await flushAsyncWork();
  });
  assert(readFullCalls() === 1, "visible full-plan surface acquires the collection");

  await act(async () => {
    root.render(
      <PlansProvider api={API}>
        <OptionalFullProbe enabled={false} />
      </PlansProvider>,
    );
    await flushAsyncWork();
  });
  await act(async () => {
    emitPlansLoaded();
    await flushAsyncWork();
  });
  assert(readFullCalls() === 1, `hidden full-plan surface remains unsubscribed after reload events (got ${readFullCalls()})`);
  await act(async () => root.unmount());
});

test("sidebar reads compact plans without mounting canvas", async () => {
  indexCalls = 0;
  fullCalls = 0;
  const { host, root } = await renderTree(
    <PlansProvider api={API}>
      <SidebarProbe />
    </PlansProvider>,
  );
  assert(readFullCalls() === 1, "sidebar requests summary metadata");
  assert(chatCalls === 0, "a sidebar or board opening never acquires the chat projection");
  assert(
    host.querySelector('[data-testid="sidebar-plan-count"]')?.textContent === "1",
    "sidebar receives plan headers",
  );
  await act(async () => root.unmount());
});

test("desktop plan queries wait for config and bypass REST", async () => {
  indexCalls = 0;
  fullCalls = 0;
  let restCalls = 0;
  const prevFetch = globalThis.fetch;
  // The installed fetch is already the mux fake daemon; this spy counts carrier HTTP calls.
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
  assert(readFullCalls() === 0, "disabled provider makes no unauthenticated full request");

  await act(async () => {
    root.render(
      <PlansProvider api={API} enabled>
        <SidebarProbe />
      </PlansProvider>,
    );
    await flushAsyncWork();
  });
  const callsAfterEnable: number = readFullCalls();
  assert(callsAfterEnable === 1, "loaded config enables summary HTTP reads only");
  assert(restCalls > 0, "desktop plan reads use authenticated HTTP");
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
  assert(readFullCalls() === 1, `shared consumers trigger one full fetch (got ${readFullCalls()})`);

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

  const callsAfterCanvasRelease: number = readFullCalls();
  assert(
    callsAfterCanvasRelease === 2,
    `sidebar refetches after canvas release (got ${readFullCalls()})`,
  );
  assert(
    host.querySelector('[data-testid="sidebar-plan-count"]')?.textContent === "1",
    "sidebar retains the summary headers after canvas release",
  );
  await act(async () => root.unmount());
});

test("workspace canvas does not rescope the shared summary query", async () => {
  indexCalls = 0;
  fullCalls = 0;
  lastFullProject = "unset";

  const { root } = await renderTree(
    <PlansProvider api={API}>
      <WorkspaceProbe projectPath="/repo/a" />
    </PlansProvider>,
  );
  assert(readFullCalls() === 1, "one scoped full fetch");
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

test("nested workspace retains its owner plan without importing another workspace plan", async () => {
  fullFixturePlans = [
    { ...FIXTURE_PLAN, id: "nested-plan", project_path: "D:\\repo", owner: { ...FIXTURE_PLAN.owner, tab_id: "nested-owner" } },
    { ...FIXTURE_PLAN, id: "other-plan", project_path: "D:\\repo", owner: { ...FIXTURE_PLAN.owner, tab_id: "other-owner" } },
  ];
  const workspaces = [
    { id: "nested", name: "Web", cwd: "D:/repo/web", tabs: ["nested-owner"] },
    { id: "other", name: "Other", cwd: "D:/repo/other", tabs: ["other-owner"] },
  ] as Workspace[];
  let selected: string[] = [];
  function CaptureNestedPlans() {
    const { plans } = useCanvasPlans("workspace", "D:/repo/web", { workspaceId: "nested", workspaces });
    selected = plans.map((plan) => plan.id);
    return null;
  }
  const { root } = await renderTree(<PlansProvider api={API}><CaptureNestedPlans /></PlansProvider>);
  assert(selected.includes("nested-plan"), "nested owner plan survives upstream project filter");
  assert(!selected.includes("other-plan"), "other workspace plan remains excluded");
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
