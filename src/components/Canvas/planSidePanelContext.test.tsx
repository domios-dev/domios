import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { reconcileDrawerBinding, selectDrawerContextKey, usePlanSidePanelContextBinding } from "./planSidePanelContext";
import { planSidePanelStore } from "./planSidePanelStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function stateOf(activeWorkspace: string | null, activeTab: string | null): ExtendedState {
  return {
    active_workspace_id: activeWorkspace,
    workspaces: [{ id: "w1", active_tab_id: activeTab }, { id: "w2", active_tab_id: "t9" }],
  } as unknown as ExtendedState;
}

describe("drawer context key", () => {
  it("changes with the active workspace and with the active tab", () => {
    const base = selectDrawerContextKey(stateOf("w1", "t1"));
    expect(selectDrawerContextKey(stateOf("w1", "t2"))).not.toBe(base);
    expect(selectDrawerContextKey(stateOf("w2", "t1"))).not.toBe(base);
    expect(selectDrawerContextKey(stateOf("w1", "t1"))).toBe(base);
    expect(selectDrawerContextKey(stateOf(null, null))).toBe(":");
  });
});

describe("reconcileDrawerBinding", () => {
  it("binds to the context the drawer opened in, keeps it there, and closes on any change", () => {
    expect(reconcileDrawerBinding(null, "a", true)).toEqual({ bound: "a", close: false });
    expect(reconcileDrawerBinding("a", "a", true)).toEqual({ bound: "a", close: false });
    expect(reconcileDrawerBinding("a", "b", true)).toEqual({ bound: null, close: true });
  });

  it("rebinds instead of closing when the context moved because a restore settled", () => {
    expect(reconcileDrawerBinding("a", "b", true, true)).toEqual({ bound: "b", close: false });
  });

  it("forgets the binding while no drawer is open, so returning never reopens one", () => {
    expect(reconcileDrawerBinding("a", "b", false)).toEqual({ bound: null, close: false });
    expect(reconcileDrawerBinding(null, "a", false)).toEqual({ bound: null, close: false });
  });
});

describe("plan and task drawer wiring", () => {
  const mounted: Array<{ root: Root; host: HTMLElement }> = [];
  afterEach(async () => {
    for (const { root, host } of mounted.splice(0)) { await act(async () => root.unmount()); host.remove(); }
    act(() => { planSidePanelStore.close(); });
  });

  function Harness({ contextKey, seeds }: { contextKey: string; seeds: number }) {
    usePlanSidePanelContextBinding(contextKey, seeds);
    return null;
  }

  async function mount(contextKey: string) {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    mounted.push({ root, host });
    let seeds = 0;
    await act(async () => root.render(<Harness contextKey={contextKey} seeds={seeds} />));
    const setContext = (next: string) => act(async () => root.render(<Harness contextKey={next} seeds={seeds} />));
    setContext.restore = (next: string) => act(async () => { seeds += 1; root.render(<Harness contextKey={next} seeds={seeds} />); });
    return setContext;
  }

  it("closes an open task drawer when the active tab changes, and stays closed on return", async () => {
    const setContext = await mount("w1:t1");
    act(() => planSidePanelStore.open({ planId: "pl_1", taskId: "task_1" }));
    expect(planSidePanelStore.get()).not.toBeNull();
    await setContext("w1:t2");
    expect(planSidePanelStore.get()).toBeNull();
    await setContext("w1:t1");
    expect(planSidePanelStore.get()).toBeNull();
  });

  it("closes the drawer on a workspace switch, and keeps it while replaced within the same context", async () => {
    const setContext = await mount("w1:t1");
    act(() => planSidePanelStore.open({ planId: "pl_1" }));
    act(() => planSidePanelStore.open({ planId: "pl_1", taskId: "task_1", nestedFromPlan: true }));
    expect(planSidePanelStore.get()?.target.taskId).toBe("task_1");
    await setContext("w2:t9");
    expect(planSidePanelStore.get()).toBeNull();
  });

  it("keeps the drawer open when the active tab changes because the restore settled, and still closes on a later user change", async () => {
    const setContext = await mount("w1:t1");
    act(() => planSidePanelStore.open({ planId: "pl_1", taskId: "task_1" }));
    await setContext.restore("w2:t9");
    expect(planSidePanelStore.get()?.target.taskId).toBe("task_1");
    await setContext("w2:t8");
    expect(planSidePanelStore.get()).toBeNull();
  });

  it("leaves a tab-bound Documents drawer to its own per-tab restore", async () => {
    const setContext = await mount("w1:t1");
    act(() => planSidePanelStore.open({ planId: "", documentsTabId: "t1" }));
    await setContext("w1:t2");
    expect(planSidePanelStore.get()?.target.documentsTabId).toBe("t1");
  });
});
