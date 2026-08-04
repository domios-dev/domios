// Run: npx tsx src/components/Canvas/usePlans.test.tsx

import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import type { UsePlansResult } from "./usePlans";

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

type ListenHandler = (ev: { payload: unknown }) => void;
const listenHandlers = new Map<string, Set<ListenHandler>>();
// Desktop plan reads are IPC into PlanStore, so the call oracle counts invokes.
let planReadCalls = 0;
let lastFullProject: string | null | undefined;
let failPlanReads = false;

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/use-plans" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function installBridgeMocks() {
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");
  __setInvokeForTest(async (command, args) => {
    planReadCalls++;
    if (failPlanReads) throw new TypeError("Load failed");
    if (command === "list_plans_full") {
      lastFullProject = (args?.project as string | null | undefined) ?? null;
      return [FIXTURE_PLAN] as never;
    }
    if (command === "get_plan_full") return FIXTURE_PLAN as never;
    if (command === "list_plan_index") return [] as never;
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

function emitPlanChanged(planId: string) {
  for (const handler of listenHandlers.get("plan-changed") ?? []) {
    handler({ payload: { plan_id: planId } });
  }
}

function emitPlansLoaded() {
  for (const handler of listenHandlers.get("plans-loaded") ?? []) {
    handler({ payload: {} });
  }
}

async function renderProbe(
  usePlans: (api: ApiConfig) => UsePlansResult,
  onUpdate: (state: UsePlansResult) => void,
): Promise<{ root: Root }> {
  function PlansProbe() {
    const state = usePlans(API);
    useEffect(() => {
      onUpdate(state);
    }, [state]);
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<PlansProbe />);
  });
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
  return { root };
}

async function main() {
  setupDom();
  await installBridgeMocks();
  const { usePlans } = await import("./usePlans");
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");

  test("usePlans loads plans on mount", async () => {
    planReadCalls = 0;
    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    if (!holder.current) throw new Error("state captured");
    assert(!holder.current.loading, "finished loading");
    assert(holder.current.plans.length === 1, "one plan loaded");
    assert(holder.current.plans[0]?.title === "Auth rollout", "plan title");
    assert(planReadCalls === 1, "one batched full plan read on load");
    await act(async () => root.unmount());
  });

  test("usePlans refetches changed plan on plan-changed", async () => {
    planReadCalls = 0;
    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    const callsAfterLoad = planReadCalls;
    emitPlanChanged(FIXTURE_PLAN.id);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    assert(planReadCalls > callsAfterLoad, "refetch after plan-changed");
    if (!holder.current) throw new Error("state captured");
    assert(holder.current.plans[0]?.id === FIXTURE_PLAN.id, "plan still present");
    await act(async () => root.unmount());
  });

  test("usePlans refetches full plans on plans-loaded", async () => {
    planReadCalls = 0;
    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    const callsAfterLoad = planReadCalls;
    await act(async () => {
      emitPlansLoaded();
      await new Promise((r) => setTimeout(r, 20));
    });
    assert(planReadCalls > callsAfterLoad, "plans-loaded refetches full plans");
    assert(holder.current?.plans[0]?.id === FIXTURE_PLAN.id, "plan still present");
    await act(async () => root.unmount());
  });

  test("usePlans passes project filter to the full plan read", async () => {
    planReadCalls = 0;
    lastFullProject = "unset";

    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(
      (api) => usePlans(api, { project: "/tmp/project-a" }),
      (state) => {
        holder.current = state;
      },
    );
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    assert(lastFullProject === "/tmp/project-a", "project scope reached the plan read");
    assert(planReadCalls === 1, "one plan read on load");
    await act(async () => root.unmount());
  });

  test("usePlans recovers after a transient plan-read failure", async () => {
    planReadCalls = 0;
    failPlanReads = true;

    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 30));
    });
    assert(holder.current?.error?.includes("Reconnecting"), "friendly error on failure");
    assert(!holder.current?.error?.includes("TypeError"), "no raw TypeError");

    failPlanReads = false;
    await act(async () => {
      await holder.current!.refetch();
    });
    assert(holder.current?.plans.length === 1, "plans loaded after recovery");
    assert(holder.current?.error === null, "error cleared");
    await act(async () => root.unmount());
  });

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
  __setInvokeForTest(null);
  __setListenForTest(null);
  if (failed > 0) process.exit(1);
}

void main();
