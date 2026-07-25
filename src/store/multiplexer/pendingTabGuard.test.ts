// Run: npx tsx src/store/multiplexer/pendingTabGuard.test.ts

import { applyPendingTabGuard, type TabGuards } from "./pendingTabGuard";
import type { MultiplexerState, Workspace, Tab } from "../../lib/types";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function tab(id: string): Tab {
  return { id, pane_id: `pane-${id}` } as unknown as Tab;
}

function ws(id: string, activeTabId: string | null, tabIds: string[]): Workspace {
  return {
    id,
    name: id,
    tabs: tabIds.map(tab),
    active_tab_id: activeTabId,
    cwd: null,
  } as unknown as Workspace;
}

function snap(workspaces: Workspace[]): MultiplexerState {
  return {
    workspaces,
    panes: {},
    active_workspace_id: workspaces[0]?.id ?? null,
    focused_pane_id: null,
    generation: 1,
  };
}

test("conflict + target present + live: masks to guarded tab, keeps guard, other fields untouched", () => {
  const s = snap([ws("w1", "A", ["A", "B"])]);
  const guards: TabGuards = { w1: { tabId: "B" } };
  const out = applyPendingTabGuard(s, guards);
  assert(out.workspaces[0].active_tab_id === "B", "active_tab_id masked to guarded B");
  assert(out !== s, "returns a new object when masking");
  assert(out.generation === 1, "generation untouched");
  assert(out.active_workspace_id === "w1", "active_workspace_id untouched");
  assert(out.workspaces[0].tabs.length === 2, "tabs untouched");
  assert(guards.w1 && guards.w1.tabId === "B", "guard retained while unconfirmed");
});

test("snapshot already shows guarded tab: not masked, guard deleted (confirmation clear)", () => {
  const s = snap([ws("w1", "B", ["A", "B"])]);
  const guards: TabGuards = { w1: { tabId: "B" } };
  const out = applyPendingTabGuard(s, guards);
  assert(out === s, "identity when nothing to mask");
  assert(out.workspaces[0].active_tab_id === "B", "server value preserved");
  assert(!("w1" in guards), "guard deleted on confirmation");
});

test("stale snapshots never expire an unconfirmed user selection", () => {
  const s = snap([ws("w1", "A", ["A", "B"])]);
  const guards: TabGuards = { w1: { tabId: "B" } };
  const out = applyPendingTabGuard(s, guards);
  assert(out.workspaces[0].active_tab_id === "B", "user selection remains active");
  assert("w1" in guards, "guard remains until explicit confirmation");
});

test("guarded target tab absent from snapshot: not masked, server value wins", () => {
  const s = snap([ws("w1", "A", ["A", "C"])]);
  const guards: TabGuards = { w1: { tabId: "B" } };
  const out = applyPendingTabGuard(s, guards);
  assert(out === s, "identity when target tab removed server-side");
  assert(out.workspaces[0].active_tab_id === "A", "server value preserved");
  assert(!("w1" in guards), "removed target clears the guard");
});

test("no guard for a workspace: snapshot returned unchanged (identity)", () => {
  const s = snap([ws("w1", "A", ["A", "B"])]);
  const guards: TabGuards = {};
  const out = applyPendingTabGuard(s, guards);
  assert(out === s, "same reference when nothing changed");
});

test("multi-workspace: guard on WS1 only masks WS1, leaves WS2 as-is", () => {
  const s = snap([ws("w1", "A", ["A", "B"]), ws("w2", "X", ["X", "Y"])]);
  const guards: TabGuards = { w1: { tabId: "B" } };
  const out = applyPendingTabGuard(s, guards);
  assert(out.workspaces[0].active_tab_id === "B", "w1 masked to guarded B");
  assert(out.workspaces[1].active_tab_id === "X", "w2 untouched");
  assert(out.workspaces[1] === s.workspaces[1], "w2 workspace object identity preserved");
});

console.log(failures === 0 ? "✓ pendingTabGuard tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
