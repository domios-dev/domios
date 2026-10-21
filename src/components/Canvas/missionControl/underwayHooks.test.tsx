import assert from "node:assert/strict";
import { test } from "node:test";
import { Window } from "happy-dom";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { __setInvokeForTest, __setListenForTest } from "../../../lib/bridge";
import { useAgentChecklists } from "../../Sidebar/useAgentChecklists";
import { useUnderwayResize } from "./useUnderwayResize";

const dom = new Window({ url: "http://localhost" });
Object.defineProperty(globalThis, "window", { value: dom, configurable: true });
Object.defineProperty(globalThis, "document", { value: dom.document, configurable: true });
Object.defineProperty(globalThis, "localStorage", { value: dom.localStorage, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
