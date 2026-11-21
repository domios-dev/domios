import { muxMockFetch } from "../../lib/muxMockFetch.testdata";
// Run: npx tsx src/components/Canvas/usePlans.test.tsx

import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import type { UsePlanTaskBoardResult, UsePlanTaskDetailResult, UsePlansResult } from "./usePlans";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): void {
  if (!condition) throw new Error(message);
}
function requirePresent<T>(value: T | null | undefined, message: string): T {
  if (value == null) throw new Error(message);
  return value;
}

const API: ApiConfig = {mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [],
  enabled: true,
  host: "127.0.0.1",
  port: 7686,
  token: "test-token",
  cors: true,
};

type ListenHandler = (ev: { payload: unknown }) => void;
const listenHandlers = new Map<string, Set<ListenHandler>>();
let wireRevision = 1;
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
  globalThis.fetch = muxMockFetch(async (input, init) => {
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
        plan_id: planId, revision: String(wireRevision),
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
          plan_id: planId, revision: String(wireRevision),
          column: { status, items: [item("task-2", 2)], next_cursor: null },
        });
      }
      return Response.json({
        plan_id: planId, revision: String(wireRevision),
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
    if (url.searchParams.get("view") === "summary") {
      lastFullProject = url.searchParams.get("project");
      return Response.json({ plans: [{ ...FIXTURE_PLAN, tasks: [], sections: [], strides: [], revision: String(wireRevision) }] });
    }
    if (url.pathname === "/api/plans") return Response.json({ plans: [] });
    if (url.pathname.startsWith("/api/plans/")) return Response.json({ ...FIXTURE_PLAN, tasks: [], sections: [], strides: [], revision: String(wireRevision) });
    throw new Error(`unexpected request: ${url}`);
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
  const previous_revision = String(wireRevision++);
  for (const handler of listenHandlers.get("plan-changed") ?? []) {
    handler({ payload: { plan_id: planId, revision: String(wireRevision), previous_revision, task_ids: ["canonical"], reset: false, deleted: false } });
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}

async function renderScopedProbe<T>(read: () => T, describe: (value: T) => string) {
  let current!: T;
  function Probe() {
    current = read();
    return <output>{describe(current)}</output>;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<Probe />); });
  return { host, root, current: () => current, unmount: async () => {
    await act(async () => root.unmount());
    host.remove();
  } };
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
    assert(planReadCalls === 1, "one batched summary projection read on load");
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

  test("usePlans refetches summary projections on plans-loaded", async () => {
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
    assert(planReadCalls > callsAfterLoad, "plans-loaded refetches summary projections");
    assert(holder.current?.plans[0]?.id === FIXTURE_PLAN.id, "plan still present");
    await act(async () => root.unmount());
  });

  test("usePlans passes project filter to the summary projection read", async () => {
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
    const readCurrent = (): UsePlanTaskBoardResult | null => current;
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
    assert(readCurrent()?.data?.counts?.pending === 2, "initial compact response includes status counts");
    assert(readCurrent()?.data?.columns?.pending?.items.length === 1, "initial response contains one bounded card");
    await act(async () => readCurrent()!.loadNextPage("pending"));
    assert(readCurrent()?.data?.columns?.pending?.items.map((task) => task.id).join(",") === "task-1,task-2", "cursor page appends without duplicating prior cards");
    assert(taskPageRequests[1]?.searchParams.get("after_task_id") === "task-1", "next request carries the stable cursor");

    failTaskPageReads = true;
    await act(async () => {
      await readCurrent()!.refetch();
      await flushMicrotasks();
    });
    assert(Boolean(readCurrent()?.error), "initial request failure is surfaced");
    assert(!readCurrent()?.error?.includes("TypeError"), "raw transport type is not exposed");
    failTaskPageReads = false;
    await act(async () => {
      await readCurrent()!.refetch();
      await flushMicrotasks();
    });
    assert(readCurrent()?.data?.plan_id === "plan-board", "retry restores the compact page");
    await act(async () => root.unmount());
    host.remove();
  });

  test("usePlanTaskBoard cancels the old plan request on selection change", async () => {
    pendingTaskPageSignal = null;
    const readPendingSignal = (): AbortSignal | null => pendingTaskPageSignal;
    let current: UsePlanTaskBoardResult | null = null;
    const readCurrent = (): UsePlanTaskBoardResult | null => current;
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
    const oldSignal = readPendingSignal();
    if (!oldSignal) throw new Error("the first page request has an abort signal");
    await act(async () => {
      root.render(<TaskBoardProbe planId="replacement-plan" />);
      await flushMicrotasks();
    });
    assert(oldSignal.aborted, "selection change aborts the previous request");
    assert(readCurrent()?.data?.plan_id === "replacement-plan", "only the selected plan page is exposed");
    await act(async () => root.unmount());
    host.remove();
  });

  test("usePlanTaskDetail reads one exact task, surfaces errors, and retries", async () => {
    taskDetailCalls = 0;
    taskDetailRequests.length = 0;
    failTaskDetailReads = false;
    let current: import("./usePlans").UsePlanTaskDetailResult | null = null;
    const readCurrent = (): import("./usePlans").UsePlanTaskDetailResult | null => current;
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
    assert(readCurrent()?.detail?.task.id === "task-exact", "exact task identity accepted");
    failTaskDetailReads = true;
    await act(async () => {
      root.render(<TaskDetailProbe planId="detail-plan" taskId="task-error" />);
      await flushMicrotasks();
    });
    assert(Boolean(readCurrent()?.error), "request error is surfaced");
    assert(!readCurrent()?.error?.includes("TypeError"), "raw transport type is not exposed");
    failTaskDetailReads = false;
    await act(async () => {
      await readCurrent()!.retry();
      await flushMicrotasks();
    });
    assert(readCurrent()?.detail?.task.id === "task-error", "retry restores exact detail");
    await act(async () => root.unmount());
    host.remove();
  });

  test("usePlanTaskDetail cancels the prior task read on selection change", async () => {
    pendingTaskDetailSignal = null;
    const readPendingSignal = (): AbortSignal | null => pendingTaskDetailSignal;
    let current: import("./usePlans").UsePlanTaskDetailResult | null = null;
    const readCurrent = (): import("./usePlans").UsePlanTaskDetailResult | null => current;
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
    const oldSignal = readPendingSignal();
    if (!oldSignal) throw new Error("the task read has an abort signal");
    await act(async () => {
      root.render(<TaskDetailProbe planId="replacement-detail-plan" />);
      await flushMicrotasks();
    });
    assert(oldSignal.aborted, "selection change aborts the previous task read");
    assert(readCurrent()?.detail?.plan_id === "replacement-detail-plan", "only the selected task detail is exposed");
    await act(async () => root.unmount());
    host.remove();
  });

  test("task board reconnect and changes reject older snapshots and cursor pages without whole-plan reads", async () => {
    const previousFetch = globalThis.fetch;
    const initial = deferred<Response>();
    const cursor = deferred<Response>();
    const requests: URL[] = [];
    const initialSignal: { current: AbortSignal | null } = { current: null };
    let deleted = false;
    const page = (name: string, rev = String(wireRevision)) => Response.json({
      plan_id: "live-board", revision: rev, counts: { pending: 1 },
      columns: { pending: { status: "pending", items: [{ id: name, name }], next_cursor: { created_at: 1, position: 1, task_id: name } } },
    });
    const initialRev = String(wireRevision);
    globalThis.fetch = muxMockFetch(async (input, init) => {
      const url = new URL(String(input));
      requests.push(url);
      if (requests.length === 1) {
        initialSignal.current = init?.signal ?? null;
        return initial.promise;
      }
      if (url.searchParams.has("status")) return cursor.promise;
      return deleted ? new Response("", { status: 404 }) : page("current");
    });
    const probe = await renderScopedProbe<UsePlanTaskBoardResult>(() => usePlanTaskBoard(API, "live-board"),
      (state) => state.data?.columns?.pending?.items.map((task) => task.name).join(",") ?? state.error ?? "Loading");
    try {
      await act(async () => { emitPlanChanged("live-board"); });
      assert(!initialSignal.current?.aborted && requests.length === 1, "same-authority change coalesces without aborting held initial board read");
      await act(async () => { initial.resolve(page("obsolete", initialRev)); });
      assert(probe.host.textContent === "current", "stale initial snapshot is rejected by revision floor and single follow-up converges");
      let pendingPage!: Promise<void>;
      await act(async () => { pendingPage = probe.current().loadNextPage("pending"); });
      const { fireResync } = await import("../../lib/resync");
      await act(async () => { fireResync(); });
      await act(async () => {
        cursor.resolve(Response.json({ plan_id: "live-board", revision: String(wireRevision), column: { status: "pending", items: [{ id: "obsolete", name: "obsolete" }], next_cursor: null } }));
        await pendingPage;
      });
      assert(probe.host.textContent === "current", "cursor from before recovery cannot resurrect a task");
      deleted = true;
      await act(async () => { emitPlanChanged("live-board"); });
      assert(probe.current().data === null && Boolean(probe.current().error), "deletion removes stale board state");
      assert(requests.length === 5, "initial, follow-up, cursor, recovery and deletion each issue one scoped request");
      assert(requests.every((url) => url.pathname === "/api/plans/live-board/tasks" && !url.searchParams.has("full")), "recovery has no whole-plan fallback");
    } finally { await probe.unmount(); globalThis.fetch = previousFetch; }
  });

  test("selected task refresh preserves readable detail and rejects late data after deletion or authority change", async () => {
    const previousFetch = globalThis.fetch;
    const pending = deferred<Response>();
    const requests: URL[] = [];
    let forbidden = false;
    const detail = (name: string) => Response.json({ plan_id: "live-detail", revision: String(wireRevision), task_id: "canonical",
      task: { id: "canonical", key: "COD-42", handle: "inspect", name }, context: { plan_id: "live-detail", sections: [] } });
    globalThis.fetch = muxMockFetch(async (input) => {
      requests.push(new URL(String(input)));
      if (forbidden) return new Response("", { status: 403 });
      return requests.length === 2 ? pending.promise : detail("Current task");
    });
    const probe = await renderScopedProbe<UsePlanTaskDetailResult>(() => usePlanTaskDetail(API, "live-detail", "cod-42"),
      (state) => state.detail?.task.name ?? state.error ?? "Loading");
    try {
      assert(probe.host.textContent === "Current task", "canonical identity is accepted for a human task key");
      await act(async () => { emitPlanChanged("unrelated"); });
      assert(requests.length === 1, "unrelated plan changes do not read the selected task");
      await act(async () => { emitPlanChanged("live-detail"); });
      assert(probe.host.textContent === "Current task", "detail stays readable during a live refresh");
      forbidden = true;
      await act(async () => { window.dispatchEvent(new window.Event("codeterm-web-boot-changed")); });
      assert(probe.current().detail === null && Boolean(probe.current().error), "lost authority clears the selected detail");
      await act(async () => { pending.resolve(detail("Obsolete task")); });
      assert(probe.current().detail === null, "late authorized response cannot restore denied state");
      forbidden = false;
      const { fireResync } = await import("../../lib/resync");
      await act(async () => { fireResync(); });
      assert(probe.host.textContent === "Current task", "recovery reloads only the selected task");
      assert(requests.every((url) => url.pathname === "/api/plans/live-detail/tasks/cod-42"), "all refreshes remain scoped to the selected task");
    } finally { await probe.unmount(); globalThis.fetch = previousFetch; }
  });

  test("summary changes reject out-of-order events and late reads, then reconnect and delete without full plans", async () => {
    const previousFetch = globalThis.fetch;
    const pending = deferred<Response>();
    const requests: URL[] = [];
    let revision = "1";
    let deleted = false;
    const summary = () => ({ ...FIXTURE_PLAN, title: `Summary ${revision}`, tasks: [], sections: [], strides: [], revision });
    globalThis.fetch = muxMockFetch(async (input) => {
      const url = new URL(String(input));
      requests.push(url);
      if (requests.length === 2) return pending.promise;
      if (url.pathname.endsWith("/projection")) return deleted ? new Response("", { status: 404 }) : Response.json(summary());
      return Response.json({ plans: deleted ? [] : [summary()] });
    });
    const emit = (value: string, previous: string, removed = false) => {
      for (const handler of listenHandlers.get("plan-changed") ?? []) handler({ payload: {
        plan_id: FIXTURE_PLAN.id, revision: value, previous_revision: previous, task_ids: ["changed"], reset: false, deleted: removed,
      } });
    };
    const probe = await renderScopedProbe(() => usePlans(API), (value) => value.plans.map((plan) => plan.title).join(","));
    try {
      await act(async () => { emit("3", "2"); });
      assert(String(probe.host.textContent) === "Summary 1", "a pending change keeps the readable summary");
      revision = "4";
      await act(async () => { emit("4", "3"); });
      assert(String(probe.host.textContent) === "Summary 1", "advancing change coalesces behind in-flight patch while keeping readable summary");
      await act(async () => { pending.resolve(Response.json({ ...summary(), title: "Obsolete", revision: "3" })); });
      assert(String(probe.host.textContent) === "Summary 4", "superseded patch response is rejected by revision floor and follow-up converges");
      const count = requests.length;
      await act(async () => { emit("2", "1"); });
      assert(requests.length === count && String(probe.host.textContent) === "Summary 4", "older events cannot issue reads or overwrite a newer summary");
      const { fireResync } = await import("../../lib/resync");
      revision = "6";
      await act(async () => { fireResync(); });
      assert(String(probe.host.textContent) === "Summary 6", "reconnect recovers a dropped change from scoped summaries");
      deleted = true;
      await act(async () => { emit("7", "6", true); });
      assert(probe.current().plans.length === 0, "deletion removes the plan and its authorized read returns no payload");
      assert(requests.every((url) => !url.searchParams.has("full") && (url.searchParams.get("view") === "summary")), "every initial, event and recovery request asks for summaries");
    } finally { await probe.unmount(); globalThis.fetch = previousFetch; }
  });

  test("usePlans coalesces advancing same and foreign changes during held list without starving unaffected plans", async () => {
    const previousFetch = globalThis.fetch;
    const firstList = deferred<Response>();
    const hotPatch = deferred<Response>();
    let activeListReads = 0;
    let maxListConcurrency = 0;
    let listRequests = 0;
    let abortedListReads = 0;
    const patchUrls: string[] = [];
    const makeSummary = (id: string, title: string, rev: string) => ({
      ...FIXTURE_PLAN, id, title, tasks: [], sections: [], strides: [], revision: rev,
    });

    globalThis.fetch = muxMockFetch(async (input, init) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/plans" && url.searchParams.get("view") === "summary") {
        listRequests += 1;
        activeListReads += 1;
        maxListConcurrency = Math.max(maxListConcurrency, activeListReads);
        init?.signal?.addEventListener("abort", () => { abortedListReads += 1; }, { once: true });
        try {
          return await firstList.promise;
        } finally {
          activeListReads -= 1;
        }
      }
      if (url.pathname.endsWith("/projection")) {
        patchUrls.push(url.pathname);
        if (url.pathname.includes("/pl_hot/")) return hotPatch.promise;
        return new Response("", { status: 404 });
      }
      throw new Error(`unexpected request: ${url}`);
    });

    const emit = (plan_id: string, rev: string, prev: string, deleted = false) => {
      for (const handler of listenHandlers.get("plan-changed") ?? []) {
        handler({ payload: { plan_id, revision: rev, previous_revision: prev, task_ids: ["t1"], reset: false, deleted } });
      }
    };

    const probe = await renderScopedProbe(
      () => usePlans(API),
      (value) => value.plans.map((plan) => `${plan.id}:${plan.title}`).sort().join(","),
    );
    try {
      assert(listRequests === 1 && activeListReads === 1, "initial list read held");
      await act(async () => {
        emit("pl_hot", "2", "1");
        emit("pl_foreign", "2", "1");
        emit("pl_hot", "3", "2");
        emit("pl_hot", "4", "3");
        emit("pl_foreign", "3", "2");
        emit("pl_hot", "5", "4");
      });
      assert(listRequests === 1 && abortedListReads === 0, "repeated advancing events do not abort held initial list read");

      await act(async () => {
        firstList.resolve(Response.json({
          plans: [makeSummary("pl_hot", "Hot rev 1", "1"), makeSummary("pl_calm", "Calm rev 1", "1")],
        }));
      });
      assert(maxListConcurrency === 1 && listRequests === 1 && abortedListReads === 0, "list read stays single-flight with zero aborts");
      assert(probe.host.textContent === "pl_calm:Calm rev 1", "unaffected plan is immediately readable while stale hot plan waits for patch");
      assert(patchUrls.filter((path) => path.includes("/pl_hot/")).length === 1, "one coalesced follow-up patch runs for pl_hot");

      await act(async () => {
        hotPatch.resolve(Response.json(makeSummary("pl_hot", "Hot rev 5", "5")));
      });
      assert(
        probe.host.textContent === "pl_calm:Calm rev 1,pl_hot:Hot rev 5",
        "hot plan converges to latest committed revision alongside unaffected plan",
      );
    } finally {
      await probe.unmount();
      globalThis.fetch = previousFetch;
    }
  });

  test("held list and board reads abort immediately on delete, epoch change, credential rotation, and plan switch", async () => {
    const previousFetch = globalThis.fetch;
    const heldList = deferred<Response>();
    let listPhase: "hold" | "after-delete" = "hold";
    const heldListSignal: { current: AbortSignal | null } = { current: null };
    const makeSummary = (id: string, title: string, rev: string) => ({
      ...FIXTURE_PLAN, id, title, tasks: [], sections: [], strides: [], revision: rev,
    });
    globalThis.fetch = muxMockFetch(async (input, init) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/plans") {
        if (listPhase === "hold") {
          heldListSignal.current = init?.signal ?? null;
          return heldList.promise;
        }
        return Response.json({ plans: [makeSummary("pl_keep", "Kept", "1")] });
      }
      return new Response("", { status: 404 });
    });

    const probe = await renderScopedProbe(
      () => usePlans(API),
      (value) => value.plans.map((plan) => plan.id).join(","),
    );
    try {
      assert(heldListSignal.current && !heldListSignal.current.aborted, "initial list read held");
      listPhase = "after-delete";
      await act(async () => {
        for (const handler of listenHandlers.get("plan-changed") ?? []) {
          handler({ payload: { plan_id: "pl_gone", revision: "2", previous_revision: "1", task_ids: [], reset: false, deleted: true } });
        }
      });
      assert(heldListSignal.current?.aborted, "actual deletion immediately aborts held pre-delete list read");
      await act(async () => {
        heldList.resolve(Response.json({
          plans: [makeSummary("pl_gone", "Deleted", "1"), makeSummary("pl_keep", "Kept", "1")],
        }));
      });
      assert(probe.host.textContent === "pl_keep", "late pre-delete list reply cannot resurrect deleted plan");
    } finally {
      await probe.unmount();
      globalThis.fetch = previousFetch;
    }

    const { usePlanBoardState } = await import("./missionControl/usePlanBoardState");
    const { FIXTURE_BOARD_READ_FIELDS } = await import("./cards/fixtures.testdata");
    const heldBoard = deferred<Response>();
    let activeHeldBoard = heldBoard;
    let boardCalls = 0;
    let boardActive = 0;
    let maxBoardConcurrency = 0;
    const boardSignal: { current: AbortSignal | null } = { current: null };
    let boardMode: "hold" | "converged" | "mismatch" | "parse-error" | "switched" = "hold";
    const boardJson = (planId: string, rev: string, total: number) => Response.json({
      ...FIXTURE_BOARD_READ_FIELDS,
      plan_id: planId,
      revision: rev,
      task_counts: { pending: total, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0, total },
      mission_control: {
        held: { held_reviews: [], open_gates: [], oldest_hold_age_ms: null, verdicts_on_held: 0 },
        bounces: { total_attempts: 0, tasks_on_second_attempt: 0, first_try_done: 0, labelled_bounced: 0 },
        event_log: { recorded_events: 0, truncated: false, entries: [] },
        areas: { state: "no_facet", facet_key: null, facet_label: null, groups: [], untagged_task_count: 0, role_fallback: [] },
        kpi: { state: "recording", not_recording_reason: null, enable_setting: null, spend: { events: 0, bytes: 0, turns: 0, tokens: { input: 0, uncached_input: 0, output: 0, total: 0 } } },
        spend: { scope: "machine", state: "recording", not_recording_reason: null, enable_setting: null, measured_tokens: 0, turns: 0, providers: [] },
      },
    });
    globalThis.fetch = muxMockFetch(async (_input, init) => {
      boardCalls += 1;
      boardActive += 1;
      maxBoardConcurrency = Math.max(maxBoardConcurrency, boardActive);
      try {
        if (boardMode === "hold") {
          boardSignal.current = init?.signal ?? null;
          return await activeHeldBoard.promise;
        }
        if (boardMode === "mismatch") return boardJson("pl_wrong", "4", 99);
        if (boardMode === "parse-error") return new Response("not-json-body", { status: 200 });
        if (boardMode === "switched") return boardJson("pl_other", "1", 7);
        return boardJson("pl_selected", "4", 42);
      } finally {
        boardActive -= 1;
      }
    });

    const latestBoard: { current: ReturnType<typeof usePlanBoardState> | null } = { current: null };
    function BoardProbe({ planId, token, rev }: { planId: string; token: string; rev: number }) {
      const state = usePlanBoardState({ ...API, token }, planId, rev);
      latestBoard.current = state;
      return <output>{state.state ? `${state.state.plan_id}:${state.state.revision}:${state.state.task_counts.total}` : state.error ?? "Loading"}</output>;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => { root.render(<BoardProbe planId="pl_selected" token="tok-1" rev={1} />); });
      assert(boardCalls === 1 && !boardSignal.current?.aborted, "initial board read held");
      await act(async () => {
        for (const handler of listenHandlers.get("plan-changed") ?? []) {
          handler({ payload: { plan_id: "pl_selected", revision: "2", previous_revision: "1", task_ids: ["t1"], reset: false, deleted: false } });
          handler({ payload: { plan_id: "pl_foreign", revision: "9", previous_revision: "8", task_ids: ["t2"], reset: false, deleted: false } });
          handler({ payload: { plan_id: "pl_selected", revision: "3", previous_revision: "2", task_ids: ["t1"], reset: false, deleted: false } });
          handler({ payload: { plan_id: "pl_selected", revision: "4", previous_revision: "3", task_ids: ["t1"], reset: false, deleted: false } });
        }
        for (const handler of listenHandlers.get("graph-worker-progress") ?? []) {
          handler({ payload: { plan_id: "pl_selected" } });
        }
        root.render(<BoardProbe planId="pl_selected" token="tok-1" rev={2} />);
      });
      assert(boardCalls === 1 && !boardSignal.current?.aborted, "same-authority board events and revision prop bumps coalesce without aborting initial read");

      boardMode = "converged";
      await act(async () => { heldBoard.resolve(boardJson("pl_selected", "1", 10)); });
      assert(maxBoardConcurrency === 1 && boardCalls === 2, "initial board read + single coalesced follow-up");
      assert(host.textContent === "pl_selected:4:42", "board converges to latest revision without publishing stale rev 1");

      boardMode = "mismatch";
      await act(async () => {
        root.render(<BoardProbe planId="pl_selected" token="tok-2" rev={2} />);
      });
      assert(latestBoard.current?.state === null && latestBoard.current?.error?.includes("did not match the selected plan"), "selected-plan identity mismatch rejects wrong payload and preserves error detail");

      boardMode = "parse-error";
      await act(async () => {
        root.render(<BoardProbe planId="pl_selected" token="tok-3" rev={2} />);
      });
      assert(latestBoard.current?.state === null && Boolean(latestBoard.current?.error) && latestBoard.current!.error !== "Could not load plans. Try again.", "non-HTTP JSON decode error keeps useful diagnostic detail");

      const heldForSwitch = deferred<Response>();
      activeHeldBoard = heldForSwitch;
      boardMode = "hold";
      await act(async () => { root.render(<BoardProbe planId="pl_selected" token="tok-4" rev={2} />); });
      const switchSignal = requirePresent(boardSignal.current, "board read held before plan switch");
      assert(!switchSignal.aborted, "board read held before plan switch");
      boardMode = "switched";
      await act(async () => { root.render(<BoardProbe planId="pl_other" token="tok-4" rev={2} />); });
      assert(switchSignal.aborted, "switching selected plan immediately aborts held read");
      await act(async () => { heldForSwitch.resolve(boardJson("pl_selected", "10", 999)); });
      assert(host.textContent === "pl_other:1:7", "late previous-plan reply cannot contaminate switched plan");
    } finally {
      await act(async () => root.unmount());
      host.remove();
      globalThis.fetch = previousFetch;
    }
  });

  test("remote peer usePlans scopes by exact peerId, coalesces, and fences switch/invalidate/late replies", async () => {
    const previousFetch = globalThis.fetch;
    const { peerEventRail } = await import("../../lib/peerEventRail");
    const { resolveTeamPlanStatus, selectRemoteTeamPlanProjection } = await import("../Sidebar/teamTaskStatus");
    const { PlansProvider, usePeerPlanSummaries } = await import("./PlansProvider");
    const heldEight = deferred<Response>();
    const heldBeforeSwitch = deferred<Response>();
    let eightPhase: "hold-initial" | "reconciled" | "genuine-empty" | "hold-switch" = "hold-initial";
    const eightSignal: { current: AbortSignal | null } = { current: null };
    const relayTargets: string[] = [];
    const relay = (data: unknown, status = 200) => Response.json({ ok: status === 200, status, data });
    const done94Summary = {
      ...FIXTURE_PLAN,
      id: "pl_rek",
      team_id: "team_sauron",
      title: "Rekap",
      revision: "94",
      tasks: Array.from({ length: 25 }, (_, i) => ({ id: `t-${i}`, name: `Task ${i}`, status: "done" })),
      sections: [],
      strides: [],
      task_counts: { total: 94, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 94, cancelled: 0 },
    };
    const emptySummary = {
      ...FIXTURE_PLAN,
      id: "pl_empty",
      team_id: "team_empty",
      title: "Empty remote effort",
      revision: "1",
      tasks: [],
      sections: [],
      strides: [],
      task_counts: { total: 0, pending: 0, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0 },
    };

    const peerListeners = new Map<string, Set<(event: import("../../lib/peerEventRail").PeerEvent) => void>>();
    const originalObserve = peerEventRail.observe;
    peerEventRail.observe = (peerId, listener) => {
      const set = peerListeners.get(peerId) ?? new Set();
      set.add(listener);
      peerListeners.set(peerId, set);
      return () => { set.delete(listener); };
    };
    const emitPeer = (peerId: string, event: import("../../lib/peerEventRail").PeerEvent) => {
      for (const listener of peerListeners.get(peerId) ?? []) listener(event);
    };

    globalThis.fetch = muxMockFetch(async (input, init) => {
      const url = new URL(String(input));
      if (url.pathname === "/api/mesh/relay-to") {
        const body = JSON.parse(String(init?.body)) as { target_instance_id: string; path: string };
        relayTargets.push(`${body.target_instance_id}:${body.path}`);
        if (body.target_instance_id === "eight") {
          if (eightPhase === "hold-initial") {
            eightSignal.current = init?.signal ?? null;
            return heldEight.promise;
          }
          if (eightPhase === "hold-switch") {
            eightSignal.current = init?.signal ?? null;
            return heldBeforeSwitch.promise;
          }
          if (eightPhase === "genuine-empty") {
            return relay({ plans: [emptySummary] });
          }
          return relay({ plans: [done94Summary] });
        }
        return relay({
          plans: [{ ...FIXTURE_PLAN, id: "pl_peer_b", team_id: "team_b", title: "Peer B", revision: "1", tasks: [], sections: [], strides: [], task_counts: { total: 3, pending: 3, in_progress: 0, in_review: 0, blocked: 0, paused: 0, done: 0, cancelled: 0 } }],
        });
      }
      return new Response("not found", { status: 404 });
    });

    const latest: { current: UsePlansResult | null } = { current: null };
    function RemoteProbe({ peerId }: { peerId: string }) {
      const state = usePlans({ ...API, peerId });
      latest.current = state;
      return <output>{state.loading ? "loading" : state.plans.map((p) => `${p.id}:${p.task_counts?.done ?? "?"}`).join(",")}</output>;
    }
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => { root.render(<RemoteProbe peerId="eight" />); });
      assert(latest.current?.loading && latest.current.plans.length === 0, "held remote summary read stays loading without inventing zeros");
      assert(resolveTeamPlanStatus(null, {}, { loading: true, unavailable: true }).state === "unavailable", "missing held remote summary stays unavailable");

      await act(async () => {
        heldEight.resolve(relay({ plans: [done94Summary] }));
      });
      assert(host.textContent === "pl_rek:94", "authoritative remote summary exposes Done 94, not loaded page 25");
      const status94 = resolveTeamPlanStatus(latest.current!.plans[0]!, {});
      assert(status94.state === "verified" && status94.counts.done === 94, "Team card resolves authoritative 94 done from remote summary");

      const truncatedWithoutCounts = { ...done94Summary, task_counts: undefined, task_window: { next_cursor: { position: 25, task_id: "t-24" } } };
      assert(
        resolveTeamPlanStatus(truncatedWithoutCounts as unknown as import("./planProjection").PlanView, {}).state === "unavailable",
        "truncated 25-of-94 task window without task_counts never fabricates 25 as total",
      );

      eightPhase = "genuine-empty";
      await act(async () => { emitPeer("eight", { type: "reconcile" }); });
      const emptyStatus = resolveTeamPlanStatus(latest.current!.plans[0]!, {});
      assert(emptyStatus.state === "verified" && emptyStatus.counts.done === 0 && emptyStatus.counts.todo === 0, "genuine empty remote summary verifies zero");

      await act(async () => { emitPeer("eight", { type: "invalidate" }); });
      assert(latest.current!.plans.length === 0 && latest.current!.loading, "peer invalidate clears prior remote summaries immediately");

      eightPhase = "reconciled";
      await act(async () => { emitPeer("eight", { type: "reconcile" }); });
      assert(host.textContent === "pl_rek:94", "peer reconcile restores authoritative remote summary");

      eightPhase = "hold-switch";
      await act(async () => { emitPeer("eight", { type: "plans-loaded" }); });
      const switchSignal = requirePresent(eightSignal.current, "remote summary read held before peer switch");
      assert(!switchSignal.aborted, "remote summary read held before peer switch");

      await act(async () => { root.render(<RemoteProbe peerId="peer-b" />); });
      assert(switchSignal.aborted, "switching peerId immediately aborts held read from eight");
      assert(host.textContent === "pl_peer_b:0", "switched peer displays its own authoritative summary");

      await act(async () => { heldBeforeSwitch.resolve(relay({ plans: [done94Summary] })); });
      assert(host.textContent === "pl_peer_b:0", "late eight reply cannot leak into peer-b");
      assert(relayTargets.some((t) => t.startsWith("eight:/api/plans?view=summary")), "remote reads targeted exact peer eight");

      // Verify the single-read consumer contract (usePeerPlanSummaries + selectRemoteTeamPlanProjection)
      const countBeforeContract = relayTargets.length;
      eightPhase = "reconciled";
      let contractSummary: ReturnType<typeof usePeerPlanSummaries> | null = null;
      function ContractProbe() {
        contractSummary = usePeerPlanSummaries("eight");
        return null;
      }
      await act(async () => {
        root.render(<PlansProvider api={API}><ContractProbe /></PlansProvider>);
        await Promise.resolve();
      });
      const eightSummaryCalls = relayTargets.slice(countBeforeContract).filter((t) => t === "eight:/api/plans?view=summary").length;
      assert(eightSummaryCalls === 1, `consumer contract issues one bounded summary read for all Teams on peer (got ${eightSummaryCalls})`);
      const matched = selectRemoteTeamPlanProjection({
        teamId: "team_sauron",
        owningPeerId: "eight",
        viewedPeerId: "eight",
        plans: contractSummary!.plans,
        available: contractSummary!.available,
        loading: contractSummary!.loading,
        reconnecting: contractSummary!.reconnecting,
        error: contractSummary!.error,
      });
      assert(matched.plan?.id === "pl_rek", "contract binds exact Team active plan on owning peer");
      const resolvedMatched = resolveTeamPlanStatus(matched.plan, {}, matched.meta);
      assert(resolvedMatched.state === "verified" && resolvedMatched.counts.done === 94, "contract yields authoritative Done 94");
      const foreignTeam = selectRemoteTeamPlanProjection({
        teamId: "team_sauron",
        owningPeerId: "third-peer",
        viewedPeerId: "eight",
        plans: contractSummary!.plans,
        available: contractSummary!.available,
        loading: contractSummary!.loading,
        reconnecting: contractSummary!.reconnecting,
        error: contractSummary!.error,
      });
      assert(foreignTeam.plan === null && foreignTeam.meta.unavailable === true, "foreign-owned Team stays unavailable instead of cross-matching");
      const missingTeam = selectRemoteTeamPlanProjection({
        teamId: "team_no_plan",
        owningPeerId: "eight",
        viewedPeerId: "eight",
        plans: contractSummary!.plans,
        available: contractSummary!.available,
        loading: contractSummary!.loading,
        reconnecting: contractSummary!.reconnecting,
        error: contractSummary!.error,
      });
      assert(missingTeam.plan === null && missingTeam.meta.unavailable === false, "confirmed absence on online peer is distinct from unavailable");

      // Source counterexample: old zero summary (updated_at 100) superseded by newer index (updated_at 200, task_count 94)
      const supersededZero = resolveTeamPlanStatus(
        { ...emptySummary, updated_at: 100 } as unknown as import("./planProjection").PlanView,
        {},
        { indexOnly: false, indexTaskCount: 94, indexOpenTaskCount: 0, indexUpdatedAt: 200, loading: false },
      );
      assert(
        supersededZero.state === "unavailable" && supersededZero.totalTasks === 94,
        "newer index task_count 94 supersedes older zero summary instead of showing verified zero",
      );
      const supersededNonzero = resolveTeamPlanStatus(
        { ...done94Summary, updated_at: 100 } as unknown as import("./planProjection").PlanView,
        {},
        { indexOnly: false, indexTaskCount: 100, indexOpenTaskCount: 6, indexUpdatedAt: 200, loading: false },
      );
      assert(
        supersededNonzero.state === "verified" && supersededNonzero.stale === true && supersededNonzero.counts.done === 94,
        "newer index ahead of older nonzero summary marks retained buckets explicitly stale",
      );
    } finally {
      peerEventRail.observe = originalObserve;
      await act(async () => root.unmount());
      host.remove();
      globalThis.fetch = previousFetch;
    }
  });

  test("selected detail skips unrelated contiguous changes, ignores old events and recovers gaps", async () => {
    const previousFetch = globalThis.fetch;
    let reads = 0;
    let revision = "10";
    globalThis.fetch = muxMockFetch(async () => { reads++; return Response.json({ plan_id: "scoped", revision, task_id: "chosen",
      task: { id: "chosen", name: `Task ${revision}` }, context: { plan_id: "scoped", sections: [] } }); });
    const emit = (value: string, previous: string, ids: string[]) => {
      for (const handler of listenHandlers.get("plan-changed") ?? []) handler({ payload: {
        plan_id: "scoped", revision: value, previous_revision: previous, task_ids: ids, reset: false, deleted: false,
      } });
    };
    const probe = await renderScopedProbe(() => usePlanTaskDetail(API, "scoped", "chosen"), (value) => value.detail?.task.name ?? "Loading");
    try {
      await act(async () => { emit("11", "10", ["other"]); });
      assert(Number(reads) === 1 && String(probe.host.textContent) === "Task 10", "an unrelated committed task does not reacquire the selected body");
      revision = "12";
      await act(async () => { emit("12", "11", ["chosen"]); });
      assert(String(probe.host.textContent) === "Task 12", "a selected entity change refreshes its own detail");
      await act(async () => { emit("11", "10", ["chosen"]); });
      assert(Number(reads) === 2, "an older event cannot issue a replacement read");
      revision = "15";
      await act(async () => { emit("15", "14", ["other"]); });
      assert(String(probe.host.textContent) === "Task 15", "a missing revision forces a scoped snapshot even when the last entity differs");
    } finally { await probe.unmount(); globalThis.fetch = previousFetch; }
  });

  test("task history loads only on demand and rejects obsolete pages after a live change", async () => {
    const previousFetch = globalThis.fetch;
    const obsolete = deferred<Response>();
    const requests: URL[] = [];
    let revision = "1";
    let historyReads = 0;
    const { TaskHistoryControls } = await import("./planView/TaskHistoryControls");
    globalThis.fetch = muxMockFetch(async (input) => {
      const url = new URL(String(input)); requests.push(url);
      if (url.pathname.endsWith("/history")) {
        historyReads++;
        if (historyReads === 1) return obsolete.promise;
        return Response.json({ plan_id: "history-plan", task_id: "chosen", revision,
          items: [{ id: "older", text: "Earlier result" }], next_cursor: null });
      }
      return Response.json({ plan_id: "history-plan", task_id: "chosen", revision,
        task: { id: "chosen", name: `Task ${revision}`, milestones: [{ id: "latest", text: `Latest ${revision}` }], execution_attempts: [] },
        context: { plan_id: "history-plan", sections: [] }, history: { milestones: 50, attempts: null } });
    });
    function HistoryProbe() {
      const state = usePlanTaskDetail(API, "history-plan", "chosen");
      return <>{state.detail && <><output>{state.detail.task.name}: {state.detail.task.milestones.map((item) => item.text).join(",")}</output>
        <TaskHistoryControls detail={state.detail} controller={state} /></>}</>;
    }
    const host = document.createElement("div"); document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<HistoryProbe />));
      assert(requests.length === 1, "opening detail does not request earlier history");
      const button = () => host.querySelector<HTMLButtonElement>("button")!;
      await act(async () => button().click());
      assert(button().disabled, "one pending history read disables its control");
      revision = "2";
      await act(async () => {
        for (const handler of listenHandlers.get("plan-changed") ?? []) handler({ payload: { plan_id: "history-plan", revision: "2", previous_revision: "1", task_ids: ["chosen"], reset: false, deleted: false } });
      });
      await act(async () => { obsolete.resolve(Response.json({ plan_id: "history-plan", task_id: "chosen", revision: "1", items: [{ id: "obsolete", text: "Obsolete history" }], next_cursor: null })); });
      assert(host.querySelector("output")?.textContent === "Task 2: Latest 2", "an old history response cannot enter the new committed detail");
      await act(async () => button().click());
      assert(host.querySelector("output")?.textContent === "Task 2: Earlier result,Latest 2", "the requested page extends visible history in order");
      assert(host.querySelector("button") === null, "the control disappears at the end of history");
      assert(requests.filter((url) => url.pathname.endsWith("/history")).every((url) => url.pathname === "/api/plans/history-plan/tasks/chosen/history" && url.searchParams.get("kind") === "milestones" && url.searchParams.get("before") === "50" && url.searchParams.get("limit") === "50"), "history requests are cursor-bounded to the selected canonical task");
    } finally { await act(async () => root.unmount()); host.remove(); globalThis.fetch = previousFetch; }
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
