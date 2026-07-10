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
let fetchCalls = 0;

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/use-plans" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async (input: RequestInfo | URL, init?: RequestInit) => {
      fetchCalls++;
      const url = String(input);
      if (url.includes("/api/plans?full=1")) {
        return new Response(JSON.stringify({ plans: [FIXTURE_PLAN] }), { status: 200 });
      }
      if (url.endsWith("/api/plans") && (!init || init.method === undefined || init.method === "GET")) {
        return new Response(JSON.stringify({ plans: [{ plan_id: FIXTURE_PLAN.id }] }), { status: 200 });
      }
      if (url.includes(`/api/plans/${FIXTURE_PLAN.id}`)) {
        return new Response(JSON.stringify(FIXTURE_PLAN), { status: 200 });
      }
      return new Response("not found", { status: 404 });
    },
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
  await installListenMock();
  const { usePlans } = await import("./usePlans");
  const { __setListenForTest } = await import("../../lib/bridge");

  test("usePlans loads plans on mount", async () => {
    fetchCalls = 0;
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
    assert(fetchCalls === 1, "one batched full fetch on load");
    await act(async () => root.unmount());
  });

  test("usePlans refetches changed plan on plan-changed", async () => {
    fetchCalls = 0;
    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    const callsAfterLoad = fetchCalls;
    emitPlanChanged(FIXTURE_PLAN.id);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    assert(fetchCalls > callsAfterLoad, "refetch after plan-changed");
    if (!holder.current) throw new Error("state captured");
    assert(holder.current.plans[0]?.id === FIXTURE_PLAN.id, "plan still present");
    await act(async () => root.unmount());
  });

  test("usePlans refetches full plans on plans-loaded", async () => {
    fetchCalls = 0;
    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    const callsAfterLoad = fetchCalls;
    await act(async () => {
      emitPlansLoaded();
      await new Promise((r) => setTimeout(r, 20));
    });
    assert(fetchCalls > callsAfterLoad, "plans-loaded refetches full plans");
    assert(holder.current?.plans[0]?.id === FIXTURE_PLAN.id, "plan still present");
    await act(async () => root.unmount());
  });

  test("usePlans passes project filter to full fetch", async () => {
    fetchCalls = 0;
    let seenProject: string | null = null;
    const prevFetch = globalThis.fetch;
    globalThis.fetch = async (input) => {
      fetchCalls++;
      const url = String(input);
      if (url.includes("/api/plans?")) {
        const parsed = new URL(url, "http://127.0.0.1");
        seenProject = parsed.searchParams.get("project");
        return new Response(JSON.stringify({ plans: [FIXTURE_PLAN] }), { status: 200 });
      }
      return prevFetch(input);
    };

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
    assert(seenProject === "/tmp/project-a", "project query param sent");
    assert(fetchCalls === 1, "one fetch on load");
    globalThis.fetch = prevFetch;
    await act(async () => root.unmount());
  });

  test("usePlans recovers after transient fetch failure", async () => {
    fetchCalls = 0;
    let fail = true;
    const prevFetch = globalThis.fetch;
    globalThis.fetch = async (input, init) => {
      fetchCalls++;
      if (fail) throw new TypeError("Load failed");
      return prevFetch(input, init);
    };

    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 30));
    });
    assert(holder.current?.error?.includes("Reconnecting"), "friendly error on failure");
    assert(!holder.current?.error?.includes("TypeError"), "no raw TypeError");

    fail = false;
    await act(async () => {
      await holder.current!.refetch();
    });
    assert(holder.current?.plans.length === 1, "plans loaded after recovery");
    assert(holder.current?.error === null, "error cleared");
    globalThis.fetch = prevFetch;
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
  __setListenForTest(null);
  if (failed > 0) process.exit(1);
}

void main();
