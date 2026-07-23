// Run: npx tsx src/store/multiplexer/pendingCloseGuard.test.ts

import { applyPendingCloseGuard, type CloseGuards } from "./pendingCloseGuard";
import type { MultiplexerState, Workspace, Tab, Pane } from "../../lib/types";

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
  const panes: Record<string, Pane> = {};
  for (const w of workspaces) {
    for (const t of w.tabs) panes[t.pane_id] = { id: t.pane_id, title: t.id } as unknown as Pane;
  }
  return {
    workspaces,
    panes,
    active_workspace_id: workspaces[0]?.id ?? null,
    focused_pane_id: null,
    generation: 1,
  };
}

// The exact interleaving from the evidence report: a higher-generation snapshot
// whose backend read raced AHEAD of the close still lists the closed pane.
test("stale snapshot still listing a locally-closed pane does not resurrect its tab", () => {
  const s = snap([ws("w1", "A", ["A", "B"])]);
  const closed: CloseGuards = new Set(["pane-B"]);
  const out = applyPendingCloseGuard(s, closed);
  assert(out !== s, "returns a new object when masking a closed pane");
  assert(
    out.workspaces[0].tabs.map((t) => t.id).join(",") === "A",
    "closed tab B is masked out of the stale snapshot",
  );
  assert(!out.panes["pane-B"], "closed pane B is removed from panes");
  assert(out.panes["pane-A"], "unrelated pane A is untouched");
  assert(closed.has("pane-B"), "tombstone retained while the pane is still reported");
});

test("confirmed removal (pane absent from snapshot) clears the tombstone, no mask", () => {
  const s = snap([ws("w1", "A", ["A"])]);
  const closed: CloseGuards = new Set(["pane-B"]);
  const out = applyPendingCloseGuard(s, closed);
  assert(out === s, "identity when nothing to mask");
  assert(!closed.has("pane-B"), "tombstone cleared once the backend confirms removal");
});

test("orphan tab referencing a closed pane is still masked while the pane object is gone", () => {
  const s = snap([ws("w1", "A", ["A", "B"])]);
  delete s.panes["pane-B"];
  const closed: CloseGuards = new Set(["pane-B"]);
  const out = applyPendingCloseGuard(s, closed);
  assert(
    out.workspaces[0].tabs.map((t) => t.id).join(",") === "A",
    "orphan tab for a closed pane is masked",
  );
  assert(closed.has("pane-B"), "tombstone retained while a tab still references the pane");
});

// Adversarial (Dumas): prove there is NO time-based clear. Repeated stale
// snapshots must keep masking forever; only an omitting snapshot may clear.
test("no time-based expiry: repeated stale snapshots keep masking, never self-clear", () => {
  const closed: CloseGuards = new Set(["pane-B"]);
  for (let i = 0; i < 100; i++) {
    const s = snap([ws("w1", "A", ["A", "B"])]);
    const out = applyPendingCloseGuard(s, closed);
    assert(
      out.workspaces[0].tabs.map((t) => t.id).join(",") === "A",
      `iteration ${i}: stale B stays masked`,
    );
    assert(closed.has("pane-B"), `iteration ${i}: tombstone never expires by elapsed calls/time`);
  }
  // Only an authoritative omission clears it.
  const confirm = snap([ws("w1", "A", ["A"])]);
  applyPendingCloseGuard(confirm, closed);
  assert(!closed.has("pane-B"), "tombstone clears only on a snapshot that omits the pane");
});

test("no tombstones: snapshot returned unchanged (identity)", () => {
  const s = snap([ws("w1", "A", ["A", "B"])]);
  const closed: CloseGuards = new Set<string>();
  const out = applyPendingCloseGuard(s, closed);
  assert(out === s, "same reference when no tombstones are active");
});

test("legit create is untouched: a fresh unrelated tab still renders", () => {
  const s = snap([ws("w1", "A", ["A", "C"])]);
  const closed: CloseGuards = new Set(["pane-B"]);
  const out = applyPendingCloseGuard(s, closed);
  assert(out === s, "tombstone for an absent pane never touches live tabs");
  assert(
    out.workspaces[0].tabs.map((t) => t.id).join(",") === "A,C",
    "newly created tab C remains visible",
  );
});

test("multi-workspace: only the workspace carrying the closed pane is rewritten", () => {
  const s = snap([ws("w1", "A", ["A", "B"]), ws("w2", "X", ["X", "Y"])]);
  const closed: CloseGuards = new Set(["pane-B"]);
  const out = applyPendingCloseGuard(s, closed);
  assert(out.workspaces[0].tabs.map((t) => t.id).join(",") === "A", "w1 masks B");
  assert(out.workspaces[1] === s.workspaces[1], "w2 object identity preserved");
});

console.log(failures === 0 ? "✓ pendingCloseGuard tests green" : `✗ ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
