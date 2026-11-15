import assert from "node:assert/strict";
import { test } from "node:test";
import { Window } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../../lib/bridge";
import { useAgentChecklists } from "../../Sidebar/useAgentChecklists";
import { useUnderwayResize } from "./useUnderwayResize";
import { UnderwayBoardRow } from "./UnderwayPanel";
import type { Tab } from "../../../lib/types";

const dom = new Window({ url: "http://localhost" });
Object.defineProperty(globalThis, "window", { value: dom, configurable: true });
Object.defineProperty(globalThis, "document", { value: dom.document, configurable: true });
Object.defineProperty(globalThis, "localStorage", { value: dom.localStorage, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("Underway keeps work visible while awaiting review and clears its marker on amendment", async () => {
  let state = "proposed";
  let revision = 1;
  let listener: ((event: { payload: unknown }) => void) | undefined;
  __setInvokeForTest((async () => ({ tab_id: "worker", generation: 1, revision, proposal: { state }, items: [{ id: "item", title: "Inspect", state: "in_progress" }] })) as never);
  __setListenForTest((async (_event: string, handler: (event: { payload: unknown }) => void) => { listener = handler; return () => {}; }) as never);
  const host = document.createElement("div"); const root = createRoot(host);
  const worker = { id: "worker", title: "Mica", role: "Worker", team_id: "team", has_checklist: true } as Tab;
  try {
    await act(async () => root.render(<UnderwayBoardRow teamId="team" plans={[]} refs={new Map()} deepLinks={{ notifications: [], tabs: { worker }, workspaces: [], scope: {}, onSelectTab: () => {} }} />));
    assert.ok(host.querySelector('[role="status"]'));
    assert.ok(host.querySelector('button[aria-label="1 of 1: Inspect, In progress"]'));
    state = "amended"; revision = 2;
    await act(async () => listener?.({ payload: { tab_id: "worker", generation: 1, revision } }));
    assert.equal(host.querySelector('[role="status"]'), null);
    assert.ok(host.querySelector('button[aria-label="1 of 1: Inspect, In progress"]'));
  } finally { await act(async () => root.unmount()); __setInvokeForTest(null); __setListenForTest(null); }
});

test("one checklist subscription updates only the changed working agent and detaches on unmount", async () => {
  const reads: string[] = [];
  let listener: ((event: { payload: unknown }) => void) | undefined;
  let stopped = false;
  let revision = 1;
  __setInvokeForTest((async (_command: string, args?: Record<string, unknown>) => {
    const id = String(args?.tabId);
    reads.push(id);
    return { tab_id: id, generation: 1, revision, items: [{ id: "item", title: `revision ${revision}`, state: "open" }] };
  }) as never);
  __setListenForTest((async (_event: string, handler: (event: { payload: unknown }) => void) => {
    assert.equal(listener, undefined);
    listener = handler;
    return () => { stopped = true; };
  }) as never);
  function Harness() {
    const views = useAgentChecklists(["a", "b"]);
    return <div>{[...views].map(([id, view]) => <p key={id}>{id}:{view.status === "ready" ? view.checklist?.items[0]?.title : view.status}</p>)}</div>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    assert.deepEqual(reads, ["a", "b"]);
    revision = 2;
    await act(async () => listener?.({ payload: { tab_id: "b", generation: 1, revision } }));
    assert.deepEqual(reads, ["a", "b", "b"]);
    assert.equal(host.textContent, "a:revision 1b:revision 2");
    await act(async () => listener?.({ payload: { tab_id: "b", generation: 1, revision } }));
    assert.equal(reads.length, 3);
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
  assert.equal(stopped, true);
});

test("resize controls clamp to the minimum and start from the default on remount", async () => {
  function Harness() {
    const { size, handle } = useUnderwayResize();
    return <div className="mc-board-live-row"><div className="mc-underway-frame"><output>{JSON.stringify(size)}</output><button type="button" {...handle}>Resize</button></div></div>;
  }
  const host = document.createElement("div");
  let root = createRoot(host);
  await act(async () => root.render(<Harness />));
  await act(async () => host.querySelector("button")?.dispatchEvent(new dom.KeyboardEvent("keydown", { bubbles: true, code: "ArrowUp" }) as unknown as Event));
  assert.equal(host.querySelector("output")?.textContent, '{"width":320,"height":200}');
  await act(async () => root.unmount());
  root = createRoot(host);
  await act(async () => root.render(<Harness />));
  assert.equal(host.querySelector("output")?.textContent, "null");
  await act(async () => root.unmount());
});

test("a large Team queues checklist reads and coalesces same-agent change bursts", async () => {
  const ids = Array.from({ length: 64 }, (_, index) => `agent-${index}`);
  const reads: string[] = [];
  let active = 0;
  let peak = 0;
  let release: (() => void) | undefined;
  const held = new Promise<void>((resolve) => { release = resolve; });
  let listener: ((event: { payload: unknown }) => void) | undefined;
  __setInvokeForTest((async (_command: string, args?: Record<string, unknown>) => {
    const id = String(args?.tabId);
    reads.push(id);
    active++;
    peak = Math.max(peak, active);
    try {
      if (reads.length === 1) await held;
      return { tab_id: id, generation: 1, revision: 4, items: [{ id: "step", title: id, state: "open" }] };
    } finally { active--; }
  }) as never);
  __setListenForTest((async (_event: string, handler: (event: { payload: unknown }) => void) => {
    listener = handler;
    return () => {};
  }) as never);
  function Harness() {
    const views = useAgentChecklists(ids);
    return <div>{[...views].filter(([, view]) => view.status === "ready").length}</div>;
  }
  const host = document.createElement("div"); const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    assert.deepEqual(reads, ["agent-0"], "a Team larger than mux admission does not dispatch every row at once");
    await act(async () => {
      for (const revision of [2, 3, 4]) listener?.({ payload: { tab_id: "agent-0", generation: 1, revision } });
    });
    assert.equal(reads.length, 1, "events do not overlap the active read");
    await act(async () => release?.());
    assert.equal(host.textContent, "64");
    assert.equal(peak, 1, "the Team uses one checklist read at a time");
    assert.equal(reads.filter((id) => id === "agent-0").length, 2, "the changed active agent has one follow-up read");
    assert.equal(reads.length, 65);
  } finally { await act(async () => root.unmount()); __setInvokeForTest(null); __setListenForTest(null); }
});
