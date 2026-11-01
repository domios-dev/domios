// Run: npx tsx src/components/Canvas/usePlans.test.tsx

import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import type { UsePlanTaskBoardResult, UsePlansResult } from "./usePlans";

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
let planReadCalls = 0;
let taskPageCalls = 0;
let lastFullProject: string | null | undefined;
let failPlanReads = false;
let failTaskPageReads = false;
let pendingTaskPageSignal: AbortSignal | null = null;
const taskPageRequests: URL[] = [];
let taskDetailCalls = 0;
let failTaskDetailReads = false;
let pendingTaskDetailSignal: AbortSignal | null = null;
const taskDetailRequests: URL[] = [];

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://127.0.0.1:7686/use-plans" });
  Object.assign(window, { __CODETERM_DAEMON__: { origin: /^https?:/.test(window.location.origin) ? window.location.origin : "http://127.0.0.1:7686", token: "test-token" } });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
}

async function installBridgeMocks() {
  const { __setListenForTest } = await import("../../lib/bridge");
  globalThis.fetch = async (input, init) => {
    planReadCalls++;
    if (failPlanReads) throw new TypeError("Load failed");
    const url = new URL(String(input));
    const taskDetailMatch = url.pathname.match(/^\/api\/plans\/([^/]+)\/tasks\/([^/]+)$/);
    if (taskDetailMatch) {
      taskDetailCalls++;
      taskDetailRequests.push(url);
      if (failTaskDetailReads) throw new TypeError("Task detail failed");
      const planId = decodeURIComponent(taskDetailMatch[1]!);
      const taskId = decodeURIComponent(taskDetailMatch[2]!);
      if (planId === "slow-detail-plan") {
        pendingTaskDetailSignal = init?.signal ?? null;
        return new Promise<Response>((_resolve, reject) => {
          pendingTaskDetailSignal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
        });
      }
      return Response.json({
        plan_id: planId,
        task_id: taskId,
        task: { id: taskId, name: taskId },
        context: { plan_id: planId, title: planId, status: "active", owner: {}, sections: [] },
      });
    }
    if (url.pathname.endsWith("/tasks")) {
      taskPageCalls++;
      taskPageRequests.push(url);
      if (failTaskPageReads) throw new TypeError("Task page failed");
      if (url.pathname.includes("slow-plan")) {
        pendingTaskPageSignal = init?.signal ?? null;
        return new Promise<Response>((_resolve, reject) => {
          pendingTaskPageSignal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
        });
      }
      const planId = decodeURIComponent(url.pathname.split("/")[3] ?? "");
      const status = url.searchParams.get("status");
      const item = (id: string, position: number) => ({
        id,
        position,
        name: id,
        status: status ?? "pending",
        created_at: position,
        updated_at: position,
        status_changed_at: position,
      });
      if (status) {
        return Response.json({
          plan_id: planId,
          column: { status, items: [item("task-2", 2)], next_cursor: null },
        });
      }
      return Response.json({
        plan_id: planId,
        counts: { pending: 2, in_progress: 0, in_review: 0, blocked: 0, done: 0, cancelled: 0, total: 2 },
        columns: {
          pending: {
            status: "pending",
            items: [item("task-1", 1)],
            next_cursor: { created_at: 1, position: 1, task_id: "task-1" },
          },
        },
      });
    }
    if (url.searchParams.get("full") === "1") {
      lastFullProject = url.searchParams.get("project");
      return Response.json({ plans: [FIXTURE_PLAN] });
    }
    if (url.pathname === "/api/plans") return Response.json({ plans: [] });
    if (url.pathname.startsWith("/api/plans/")) return Response.json(FIXTURE_PLAN);
    throw new Error(`unexpected request: ${url}`);
  };
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
    await Promise.resolve();
  });
  return { root };
}

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

async function main() {
  setupDom();
  await installBridgeMocks();
  const { usePlanTaskBoard, usePlanTaskDetail, usePlans } = await import("./usePlans");
  const { __setInvokeForTest, __setListenForTest } = await import("../../lib/bridge");

  test("usePlans loads plans on mount", async () => {
    planReadCalls = 0;
    const holder: { current: UsePlansResult | null } = { current: null };
    const { root } = await renderProbe(usePlans, (state) => {
      holder.current = state;
    });
    await act(async () => {
      await Promise.resolve();
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
      await Promise.resolve();
    });
    const callsAfterLoad = planReadCalls;
    emitPlanChanged(FIXTURE_PLAN.id);
    await act(async () => {
      await Promise.resolve();
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
      await Promise.resolve();
    });
    const callsAfterLoad = planReadCalls;
    await act(async () => {
      emitPlansLoaded();
      await Promise.resolve();
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
      await Promise.resolve();
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
      await Promise.resolve();
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

  test("usePlanTaskBoard pages by cursor and retries a real request error", async () => {
    taskPageCalls = 0;
    taskPageRequests.length = 0;
    failTaskPageReads = false;
    let current: UsePlanTaskBoardResult | null = null;
    function TaskBoardProbe({ planId }: { planId: string }) {
      const state = usePlanTaskBoard(API, planId);
      useEffect(() => { current = state; }, [state]);
      return null;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(<TaskBoardProbe planId="plan-board" />);
      await flushMicrotasks();
    });
    assert(current?.data?.counts?.pending === 2, "initial compact response includes status counts");
    assert(current?.data?.columns?.pending?.items.length === 1, "initial response contains one bounded card");
    await act(async () => current!.loadNextPage("pending"));
    assert(current?.data?.columns?.pending?.items.map((task) => task.id).join(",") === "task-1,task-2", "cursor page appends without duplicating prior cards");
    assert(taskPageRequests[1]?.searchParams.get("after_task_id") === "task-1", "next request carries the stable cursor");

    failTaskPageReads = true;
    await act(async () => {
      await current!.refetch();
      await flushMicrotasks();
    });
    assert(Boolean(current?.error), "initial request failure is surfaced");
    assert(!current?.error?.includes("TypeError"), "raw transport type is not exposed");
    failTaskPageReads = false;
    await act(async () => {
      await current!.refetch();
      await flushMicrotasks();
    });
    assert(current?.data?.plan_id === "plan-board", "retry restores the compact page");
    await act(async () => root.unmount());
    host.remove();
  });

  test("usePlanTaskBoard cancels the old plan request on selection change", async () => {
    pendingTaskPageSignal = null;
    let current: UsePlanTaskBoardResult | null = null;
    function TaskBoardProbe({ planId }: { planId: string }) {
      const state = usePlanTaskBoard(API, planId);
      useEffect(() => { current = state; }, [state]);
      return null;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(<TaskBoardProbe planId="slow-plan" />);
      await Promise.resolve();
    });
    const oldSignal = pendingTaskPageSignal;
    assert(oldSignal, "the first page request has an abort signal");
    await act(async () => {
      root.render(<TaskBoardProbe planId="replacement-plan" />);
      await flushMicrotasks();
    });
    assert(oldSignal.aborted, "selection change aborts the previous request");
    assert(current?.data?.plan_id === "replacement-plan", "only the selected plan page is exposed");
    await act(async () => root.unmount());
    host.remove();
  });

  test("usePlanTaskDetail reads one exact task, surfaces errors, and retries", async () => {
    taskDetailCalls = 0;
    taskDetailRequests.length = 0;
    failTaskDetailReads = false;
    let current: import("./usePlans").UsePlanTaskDetailResult | null = null;
    function TaskDetailProbe({ planId, taskId }: { planId: string; taskId: string }) {
      const state = usePlanTaskDetail(API, planId, taskId);
      useEffect(() => { current = state; }, [state]);
      return null;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(<TaskDetailProbe planId="detail-plan" taskId="task-exact" />);
      await flushMicrotasks();
    });
    assert(taskDetailCalls === 1, "one task detail request");
    assert(taskDetailRequests[0]?.pathname === "/api/plans/detail-plan/tasks/task-exact", "exact scoped task path");
    assert(current?.detail?.task.id === "task-exact", "exact task identity accepted");
    failTaskDetailReads = true;
    await act(async () => {
      root.render(<TaskDetailProbe planId="detail-plan" taskId="task-error" />);
      await flushMicrotasks();
    });
    assert(Boolean(current?.error), "request error is surfaced");
    assert(!current?.error?.includes("TypeError"), "raw transport type is not exposed");
    failTaskDetailReads = false;
    await act(async () => {
      await current!.retry();
      await flushMicrotasks();
    });
    assert(current?.detail?.task.id === "task-error", "retry restores exact detail");
    await act(async () => root.unmount());
    host.remove();
  });

  test("usePlanTaskDetail cancels the prior task read on selection change", async () => {
    pendingTaskDetailSignal = null;
    let current: import("./usePlans").UsePlanTaskDetailResult | null = null;
    function TaskDetailProbe({ planId }: { planId: string }) {
      const state = usePlanTaskDetail(API, planId, "task-current");
      useEffect(() => { current = state; }, [state]);
      return null;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(<TaskDetailProbe planId="slow-detail-plan" />);
      await Promise.resolve();
    });
    const oldSignal = pendingTaskDetailSignal;
    assert(oldSignal, "the task read has an abort signal");
    await act(async () => {
      root.render(<TaskDetailProbe planId="replacement-detail-plan" />);
      await flushMicrotasks();
    });
    assert(oldSignal.aborted, "selection change aborts the previous task read");
    assert(current?.detail?.plan_id === "replacement-detail-plan", "only the selected task detail is exposed");
    await act(async () => root.unmount());
    host.remove();
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
