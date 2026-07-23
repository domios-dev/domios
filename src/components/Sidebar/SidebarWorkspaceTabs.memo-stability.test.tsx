// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.memo-stability.test.tsx
//
// Render-count oracle for FIX 2 (restore TabItem memo stability on a status flip).
//
// Two things are asserted:
//   1. The memo SEAM. TabItem ships as memo(TabItemInner, tabItemPropsEqual). This
//      harness reproduces that seam with a render-counting stand-in and drives the
//      exact prop pattern SidebarWorkspaceTabs/SidebarTabItemRow emit (fresh inline
//      onClick/onClose/onRename closures every render + per-render item arrays).
//      BEFORE (naive React.memo, default shallow compare): flipping ONE pane's
//      status re-renders EVERY row, because the fresh callbacks fail the shallow
//      check. AFTER (custom comparator mirroring tabItemPropsEqual, which ignores
//      callback identity, + stable item arrays): only the flipped row re-renders.
//   2. The REAL SidebarWorkspaceTabs mounts, and a status flip updates only the
//      target row's status while leaving the tab set intact (observable end-state).

import { readFileSync } from "node:fs";
import { act, memo, useState } from "react";
import { Window } from "happy-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Pane, PaneStatus, Tab, Team, Workspace } from "../../lib/types";
import { SidebarDnd } from "./sortable";
import { SidebarWorkspaceTabs } from "./SidebarWorkspaceTabs";
import { __setInvokeForTest } from "../../lib/bridge";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) { tests.push([name, fn]); }
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of ["window", "document", "navigator", "HTMLElement", "SVGElement", "Element", "Node", "Event", "CustomEvent", "MouseEvent", "KeyboardEvent"] as const) {
    Object.defineProperty(globalThis, key, { value: (window as never)[key], configurable: true });
  }
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: (cb: FrameRequestCallback) => { cb(0); return 0; } });
  Object.defineProperty(globalThis, "cancelAnimationFrame", { configurable: true, value: () => {} });
}

// ---------------------------------------------------------------------------
// Part 1 — the memo seam, reproduced with a render-counting stand-in.
// ---------------------------------------------------------------------------

interface RowProps {
  id: string;
  status: PaneStatus | null;
  pane: Pane;
  // Callbacks are intentionally unstable per render, exactly like SidebarTabItemRow's
  // `onClick={() => onActivateTab(tab.id)}` / `onRename={(t) => onRenameTab(tab.id, t)}`.
  onClick: () => void;
  onClose: () => void;
  onRename: (title: string) => void;
  // Per-render array identity, exactly like the looseItems fed into SortableContext.
  items: string[];
}

function TabBody({ id, status }: RowProps) {
  const counts = (globalThis as { __tabRenderCounts?: Record<string, number> }).__tabRenderCounts ??= {};
  counts[id] = (counts[id] ?? 0) + 1;
  return <div data-status={status ?? "none"}>{id}</div>;
}

// Default shallow compare — the pre-fix regression: unstable callbacks defeat it.
const NaiveTab = memo(TabBody);

// Mirrors the RELEVANT subset of TabItem.tsx's tabItemPropsEqual: compare status
// and pane identity, IGNORE onClick/onClose/onRename/items identity.
function seamEqual(prev: RowProps, next: RowProps): boolean {
  return prev.status === next.status && prev.pane === next.pane;
}
const SeamTab = memo(TabBody, seamEqual);

function makeRowProps(id: string, status: PaneStatus | null, pane: Pane): RowProps {
  return {
    id,
    status,
    pane,
    onClick: () => {},
    onClose: () => {},
    onRename: () => {},
    items: [`tab-${id}-a`, `tab-${id}-b`], // fresh identity every render
  };
}

async function measureSeam(
  Variant: typeof NaiveTab,
  panes: Record<string, Pane>,
  flipId: string,
): Promise<{ base: Record<string, number>; deltas: Record<string, number> }> {
  (globalThis as { __tabRenderCounts?: Record<string, number> }).__tabRenderCounts = {};
  let setStatuses: (fn: (prev: Record<string, PaneStatus>) => Record<string, PaneStatus>) => void = () => {};
  function List() {
    const [statuses, setS] = useState<Record<string, PaneStatus>>(() =>
      Object.fromEntries(Object.keys(panes).map((id) => [id, "Idle" as PaneStatus])),
    );
    setStatuses = setS;
    return (
      <>
        {Object.keys(panes).map((id) => (
          <Variant key={id} {...makeRowProps(id, statuses[id] ?? null, panes[id])} />
        ))}
      </>
    );
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(<List />); });
  const base = { ...((globalThis as { __tabRenderCounts?: Record<string, number> }).__tabRenderCounts ?? {}) };
  await act(async () => {
    setStatuses((prev) => ({ ...prev, [flipId]: "Working" }));
  });
  const after = (globalThis as { __tabRenderCounts?: Record<string, number> }).__tabRenderCounts ?? {};
  const deltas: Record<string, number> = {};
  for (const id of Object.keys(panes)) deltas[id] = (after[id] ?? 0) - (base[id] ?? 0);
  await act(async () => root.unmount());
  return { base, deltas };
}

// ---------------------------------------------------------------------------
// Part 2 — real SidebarWorkspaceTabs observable end-state.
// ---------------------------------------------------------------------------

function pane(overrides: Partial<Pane> & { team_id?: string | null }): Pane {
  return {
    id: "pane", pty_id: null, pane_type: "Terminal", title: "Pane", cwd: null,
    role: null, role_profile: null, parent_pane_id: null, ...overrides,
  } as Pane;
}
function tab(id: string, paneId: string, sortOrder: number): Tab {
  return { id, pane_id: paneId, sort_order: sortOrder };
}
const TEAM: Team = { id: "team-x", name: "T", lifecycle: "active", created_at: 1, workspace_id: "workspace" };

async function renderReal(
  panes: Record<string, Pane>,
  tabs: Tab[],
  statuses: Record<string, PaneStatus>,
): Promise<{ host: HTMLDivElement; root: Root }> {
  const workspace: Workspace = {
    id: "workspace", name: "Workspace", tabs, active_tab_id: tabs[0]?.id ?? null, cwd: "/workspace",
  };
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
        <SidebarWorkspaceTabs
          workspace={workspace}
          activeTabId={workspace.active_tab_id}
          sortedTabs={tabs}
          panes={panes}
          paneStatuses={statuses}
          teams={[TEAM]}
          planIndex={[]}
          plans={[]}
          activeWork={null}
          remoteWorkers={{}}
          providers={[]}
          closingTabIds={new Set()}
          appearingTabIds={new Set()}
          isActiveVisual={true}
          workspaceCanvasActive={false}
          touchMode={true}
          compact={false}
          sidebarActionText="text-xs"
          orchestrators={[]}
          onBeforeOpenPlan={() => {}}
          onActivateTab={() => {}}
          onCloseTab={() => {}}
          onRenameTab={() => {}}
          onNewSession={() => {}}
        />
      </SidebarDnd>,
    );
  });
  return { host, root };
}

async function main() {
  setupDom();
  Object.defineProperty(globalThis, "requestAnimationFrame", { configurable: true, value: (cb: FrameRequestCallback) => { cb(0); return 0; } });
  __setInvokeForTest((async () => ({})) as never);
  // globals.css is read by the real component's imports in other tests; harmless here.
  try { readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8"); } catch { /* optional */ }

  const N = 6;
  const panes: Record<string, Pane> = {};
  for (let i = 0; i < N; i++) panes[`p${i}`] = pane({ id: `p${i}`, title: `Tab ${i}`, agent_type: "claude" });
  const flipId = "p3";

  test("naive memo (pre-fix seam): one status flip re-renders EVERY row", async () => {
    const { deltas } = await measureSeam(NaiveTab, panes, flipId);
    const total = Object.values(deltas).reduce((a, b) => a + b, 0);
    const others = Object.entries(deltas).filter(([id]) => id !== flipId).map(([, v]) => v);
    console.log(`  naive: deltas ${JSON.stringify(deltas)} (total ${total})`);
    assert(deltas[flipId] === 1, `flipped row re-renders once (got ${deltas[flipId]})`);
    assert(others.every((v) => v === 1), `pre-fix baseline: all other rows re-render (got ${JSON.stringify(deltas)})`);
    assert(total === N, `pre-fix: all ${N} rows re-render (got ${total})`);
  });

  test("memo seam (post-fix): one status flip re-renders ONLY the changed row", async () => {
    const { deltas } = await measureSeam(SeamTab, panes, flipId);
    const total = Object.values(deltas).reduce((a, b) => a + b, 0);
    const others = Object.entries(deltas).filter(([id]) => id !== flipId).map(([, v]) => v);
    console.log(`  seam: deltas ${JSON.stringify(deltas)} (total ${total})`);
    assert(deltas[flipId] === 1, `flipped row re-renders once (got ${deltas[flipId]})`);
    assert(others.every((v) => v === 0), `post-fix: no other row re-renders (got ${JSON.stringify(deltas)})`);
    assert(total === 1, `post-fix: exactly one row re-renders (got ${total})`);
  });

  test("real SidebarWorkspaceTabs: status flip updates only target row, tab set intact", async () => {
    const tabs = Object.keys(panes).map((id, i) => tab(`tab-${id}`, id, i));
    const idle: Record<string, PaneStatus> = Object.fromEntries(Object.keys(panes).map((id) => [id, "Idle"]));
    const { host, root } = await renderReal(panes, tabs, idle);
    const before = host.querySelectorAll('[data-testid="tab-item"]').length;
    assert(before === N, `renders ${N} tab rows (got ${before})`);

    await act(async () => {
      root.render(
        <SidebarDnd handlers={{ reorderTabs: () => {}, reorderWorkspaces: () => {} }}>
          <SidebarWorkspaceTabs
            workspace={{ id: "workspace", name: "Workspace", tabs, active_tab_id: tabs[0].id, cwd: "/workspace" }}
            activeTabId={tabs[0].id}
            sortedTabs={tabs}
            panes={panes}
            paneStatuses={{ ...idle, [flipId]: "Working" }}
            teams={[TEAM]} planIndex={[]} plans={[]} activeWork={null} remoteWorkers={{}} providers={[]}
            closingTabIds={new Set()} appearingTabIds={new Set()} isActiveVisual={true}
            workspaceCanvasActive={false} touchMode={true} compact={false} sidebarActionText="text-xs"
            orchestrators={[]} onBeforeOpenPlan={() => {}} onActivateTab={() => {}} onCloseTab={() => {}}
            onRenameTab={() => {}} onNewSession={() => {}}
          />
        </SidebarDnd>,
      );
    });
    const after = host.querySelectorAll('[data-testid="tab-item"]').length;
    assert(after === N, `tab set intact after status flip (got ${after})`);
    console.log(`  real: ${after} rows stable across status flip`);
    await act(async () => root.unmount());
  });

  let failed = 0;
  for (const [name, fn] of tests) {
    try { await fn(); console.log(`PASS ${name}`); }
    catch (error) { failed += 1; console.error(`FAIL ${name}`, error); }
  }
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
